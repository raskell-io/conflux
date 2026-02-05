//! Daemon mode - runs the API server.

use crate::config::Config;
use anyhow::{Context, Result};
use clap::Args;
use conflux_api::{AppState, ServerConfig};
use conflux_core::{Clock, Document};
use conflux_git::{MilestoneProjector, OutputFormat, ProjectorConfig};
use conflux_replication::{
    anti_entropy::AntiEntropyTask, client::PeerManager, leader::LeaderElectionTask,
    proto::replication_service_server::ReplicationServiceServer, service::ReplicationServiceImpl,
    ClusterConfig as ReplicationClusterConfig, ClusterState, NodeId, PeerConfig as ReplicationPeerConfig,
    ReconnectTask,
};
use conflux_schema::Schema;
use conflux_store::SqliteStore;
use parking_lot::Mutex;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tonic::transport::Server;

/// Run the Conflux daemon (API server).
#[derive(Debug, Args)]
pub struct DaemonArgs {
    /// HTTP listen address (overrides config).
    #[arg(long)]
    pub host: Option<String>,

    /// HTTP listen port (overrides config).
    #[arg(long, short)]
    pub port: Option<u16>,

    /// Enable debug logging.
    #[arg(long, short)]
    pub debug: bool,
}

/// Runs the daemon command.
pub async fn run(args: DaemonArgs, config: &Config, config_dir: &Path) -> Result<()> {
    // Initialize logging
    let log_level = if args.debug { "debug" } else { "info" };
    let env_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| log_level.to_string());

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .init();

    tracing::info!("Starting Conflux daemon");

    // Load schema
    let schema_path = config_dir.join(&config.schema);
    let schema = Schema::from_file(&schema_path)
        .with_context(|| format!("loading schema from {}", schema_path.display()))?;
    tracing::info!("Loaded schema: {} v{}", schema.name, schema.version);

    // Open store
    let db_path = config_dir.join(&config.database);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("invalid database path"))?;
    let store = SqliteStore::open(db_path_str)?;
    tracing::info!("Opened database: {}", db_path.display());

    // Create clock (with node ID if clustering enabled)
    let clock = if let Some(ref cluster_config) = config.cluster {
        Clock::with_node_id(&cluster_config.node_id)
            .with_context(|| format!("creating clock for node {}", cluster_config.node_id))?
    } else {
        Clock::new()
    };

    // Load document state from store
    let mut document = Document::new();
    let schema_info = schema.as_schema_info();

    let stored_ops = store.query_operations(
        &conflux_store::OperationQuery::new(&config.document_id).limit(100000),
    )?;
    let op_count = stored_ops.len();
    for stored in stored_ops {
        let _ = document.apply(&stored.operation, &schema_info, &clock);
    }
    tracing::info!(
        "Loaded {} operations for document '{}'",
        op_count,
        config.document_id
    );

    // Configure git projector if available
    let projector = if let Some(ref git_config) = config.git {
        let repo_path = config_dir.join(&git_config.repo);
        let format = parse_format(&git_config.format)?;
        let projector_config = ProjectorConfig {
            repo_path,
            default_format: format,
            author_name: git_config
                .author_name
                .clone()
                .unwrap_or_else(|| "Conflux".to_string()),
            author_email: git_config
                .author_email
                .clone()
                .unwrap_or_else(|| "conflux@localhost".to_string()),
        };
        let p = MilestoneProjector::new(projector_config);
        tracing::info!("Git projection enabled: {}", git_config.repo.display());
        Some(Arc::new(p))
    } else {
        tracing::info!("Git projection not configured");
        None
    };

    // Build app state
    let state = AppState::new_with_projector(schema, store, &config.document_id, projector);

    // Apply existing document state
    {
        let mut doc = state.document.write().unwrap();
        *doc = document;
    }

    // Configure server
    let host = args
        .host
        .as_deref()
        .unwrap_or(&config.server.host)
        .to_string();
    let port = args.port.unwrap_or(config.server.port);
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("parsing address {host}:{port}"))?;

    let server_config = ServerConfig::new().with_http_addr(addr);

    tracing::info!("API server listening on http://{}", addr);
    tracing::info!("Document ID: {}", config.document_id);

    // Initialize replication if cluster config exists
    if let Some(ref cluster_config) = config.cluster {
        let replication_config = build_replication_config(cluster_config);
        let cluster_state = Arc::new(ClusterState::new(&replication_config));
        let clock = Arc::new(Clock::with_node_id(&cluster_config.node_id)?);
        let store = Arc::new(Mutex::new(SqliteStore::open(db_path_str)?));

        tracing::info!(
            "Cluster mode enabled: node_id={}, peers={}",
            cluster_config.node_id,
            cluster_config.peers.len()
        );

        // Create peer manager and connect to peers
        let peer_manager = Arc::new(PeerManager::new(cluster_state.clone()));

        // Spawn replication gRPC server
        let replication_addr: SocketAddr = format!("0.0.0.0:{}", cluster_config.replication_port)
            .parse()
            .with_context(|| {
                format!(
                    "parsing replication address 0.0.0.0:{}",
                    cluster_config.replication_port
                )
            })?;

        let replication_service = ReplicationServiceImpl::new(
            cluster_state.clone(),
            clock.clone(),
            store.clone(),
        );

        let grpc_server = Server::builder()
            .add_service(ReplicationServiceServer::new(replication_service))
            .serve(replication_addr);

        tracing::info!(
            "Replication gRPC server listening on {}",
            replication_addr
        );

        // Spawn background tasks

        // Initial connection to peers
        let peer_manager_connect = peer_manager.clone();
        tokio::spawn(async move {
            peer_manager_connect.connect_all().await;
        });

        // Reconnection task with exponential backoff
        let reconnect_task = ReconnectTask::new(
            peer_manager.clone(),
            std::time::Duration::from_secs(10),
        );
        tokio::spawn(reconnect_task.run());

        let leader_task = LeaderElectionTask::new(
            cluster_state.clone(),
            peer_manager.clone(),
            clock.clone(),
            &replication_config,
        );
        tokio::spawn(leader_task.run());

        let anti_entropy_task = AntiEntropyTask::new(
            cluster_state.clone(),
            peer_manager.clone(),
            store.clone(),
            clock.clone(),
            &config.document_id,
            &replication_config,
        );
        tokio::spawn(anti_entropy_task.run());

        // Run both servers concurrently
        tokio::select! {
            result = conflux_api::run_server(server_config, state) => {
                result?;
            }
            result = grpc_server => {
                result.with_context(|| "replication gRPC server failed")?;
            }
        }
    } else {
        tracing::info!("Single-node mode (no cluster config)");
        tracing::info!("Press Ctrl+C to stop");

        // Run server
        conflux_api::run_server(server_config, state).await?;
    }

    Ok(())
}

/// Builds a replication ClusterConfig from the CLI ClusterConfig.
fn build_replication_config(cli_config: &crate::config::ClusterConfig) -> ReplicationClusterConfig {
    let mut config = ReplicationClusterConfig::new(cli_config.node_id.as_str())
        .with_replication_port(cli_config.replication_port);

    config.anti_entropy_interval_secs = cli_config.anti_entropy_interval_secs;
    config.heartbeat_interval_secs = cli_config.heartbeat_interval_secs;
    config.peer_timeout_secs = cli_config.peer_timeout_secs;

    for peer in &cli_config.peers {
        config.peers.push(ReplicationPeerConfig {
            node_id: NodeId::new(peer.node_id.as_str()),
            addr: peer.addr.clone(),
        });
    }

    config
}

fn parse_format(s: &str) -> Result<OutputFormat> {
    match s.to_lowercase().as_str() {
        "yaml" | "yml" => Ok(OutputFormat::Yaml),
        "json" => Ok(OutputFormat::Json),
        "toml" => Ok(OutputFormat::Toml),
        "kdl" => Ok(OutputFormat::Kdl),
        "xml" => Ok(OutputFormat::Xml),
        "hcl" | "tf" => Ok(OutputFormat::Hcl),
        _ => anyhow::bail!("unknown output format: {s}"),
    }
}
