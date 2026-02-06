//! RBAC management and checking commands.

use anyhow::Result;
use clap::{Args, Subcommand};
use conflux_core::{ActorClass, ActorId};
use conflux_rbac::{Action, AuthzContext, Authorizer, RbacAuthorizer, RbacConfig, Resource};
use std::path::PathBuf;

/// RBAC management commands.
#[derive(Debug, Args)]
pub struct RbacArgs {
    #[command(subcommand)]
    pub command: RbacCommands,
}

#[derive(Debug, Subcommand)]
pub enum RbacCommands {
    /// List all defined roles.
    ListRoles(ListRolesArgs),

    /// Check if an action is allowed for an actor.
    Check(CheckArgs),

    /// Show roles assigned to an actor.
    ShowRoles(ShowRolesArgs),

    /// Validate an RBAC configuration file.
    Validate(ValidateArgs),
}

/// List roles.
#[derive(Debug, Args)]
pub struct ListRolesArgs {
    /// Path to the RBAC configuration file.
    #[arg(long, default_value = "rbac.toml")]
    pub config: PathBuf,

    /// Output format (table, json).
    #[arg(long, default_value = "table")]
    pub format: String,
}

/// Check permissions.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Actor ID to check.
    #[arg(long)]
    pub actor: String,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long, default_value = "human")]
    pub actor_class: String,

    /// Action to check.
    #[arg(long)]
    pub action: String,

    /// Resource to check (pattern syntax).
    #[arg(long)]
    pub resource: String,

    /// Path to the RBAC configuration file.
    #[arg(long, default_value = "rbac.toml")]
    pub config: PathBuf,
}

/// Show roles for an actor.
#[derive(Debug, Args)]
pub struct ShowRolesArgs {
    /// Actor ID.
    #[arg(long)]
    pub actor: String,

    /// Actor class (human, pipeline, operator, system).
    #[arg(long, default_value = "human")]
    pub actor_class: String,

    /// Path to the RBAC configuration file.
    #[arg(long, default_value = "rbac.toml")]
    pub config: PathBuf,
}

/// Validate RBAC config.
#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Path to the RBAC configuration file.
    #[arg(long, default_value = "rbac.toml")]
    pub config: PathBuf,
}

/// Run the RBAC command.
pub fn run(args: RbacArgs) -> Result<()> {
    match args.command {
        RbacCommands::ListRoles(list_args) => run_list_roles(list_args),
        RbacCommands::Check(check_args) => run_check(check_args),
        RbacCommands::ShowRoles(show_args) => run_show_roles(show_args),
        RbacCommands::Validate(validate_args) => run_validate(validate_args),
    }
}

fn run_list_roles(args: ListRolesArgs) -> Result<()> {
    let config = RbacConfig::from_file(&args.config)?;

    match args.format.as_str() {
        "json" => {
            let output = serde_json::to_string_pretty(&config.role)?;
            println!("{}", output);
        }
        _ => {
            println!("{:<20} {:<15} DESCRIPTION", "ROLE", "INHERITS");
            println!("{}", "-".repeat(60));

            for (name, role) in &config.role {
                let inherits = if role.inherits.is_empty() {
                    "-".to_string()
                } else {
                    role.inherits.join(", ")
                };
                let description = role.description.as_deref().unwrap_or("-");
                println!("{:<20} {:<15} {}", name, inherits, description);
            }
        }
    }

    Ok(())
}

fn run_check(args: CheckArgs) -> Result<()> {
    let config = RbacConfig::from_file(&args.config)?;
    let authorizer = RbacAuthorizer::new(config);

    let actor_class = parse_actor_class(&args.actor_class)?;
    let actor = ActorId::new(&args.actor, actor_class);
    let action = parse_action(&args.action)?;
    let resource = parse_resource(&args.resource)?;

    let ctx = AuthzContext::new(actor, action, resource);
    let result = authorizer.authorize(&ctx);

    match result {
        conflux_rbac::AuthzResult::Allowed => {
            println!("✓ ALLOWED");
            println!("  Actor '{}' can {} on {}", args.actor, args.action, args.resource);
        }
        conflux_rbac::AuthzResult::Denied(reason) => {
            println!("✗ DENIED");
            println!("  {}", reason);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn run_show_roles(args: ShowRolesArgs) -> Result<()> {
    let config = RbacConfig::from_file(&args.config)?;
    let authorizer = RbacAuthorizer::new(config);

    let actor_class = parse_actor_class(&args.actor_class)?;
    let actor = ActorId::new(&args.actor, actor_class);

    let roles = authorizer.resolver().roles_for_actor(&actor);

    if roles.is_empty() {
        println!("Actor '{}' has no roles assigned", args.actor);
    } else {
        println!("Roles for actor '{}':", args.actor);
        for role in &roles {
            println!("  - {}", role);
        }
    }

    Ok(())
}

fn run_validate(args: ValidateArgs) -> Result<()> {
    match RbacConfig::from_file(&args.config) {
        Ok(config) => {
            println!("✓ Configuration is valid");
            println!("  {} roles defined", config.role.len());
            println!("  {} assignments defined", config.assignment.len());
        }
        Err(e) => {
            println!("✗ Configuration is invalid");
            println!("  {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

fn parse_actor_class(s: &str) -> Result<ActorClass> {
    match s.to_lowercase().as_str() {
        "human" => Ok(ActorClass::Human),
        "pipeline" => Ok(ActorClass::Pipeline),
        "operator" => Ok(ActorClass::Operator),
        "system" => Ok(ActorClass::System),
        _ => anyhow::bail!(
            "unknown actor class: {} (expected: human, pipeline, operator, system)",
            s
        ),
    }
}

fn parse_action(s: &str) -> Result<Action> {
    match s.to_lowercase().as_str() {
        "read" => Ok(Action::Read),
        "write" => Ok(Action::Write),
        "create" => Ok(Action::Create),
        "delete" => Ok(Action::Delete),
        "move" => Ok(Action::Move),
        "override" => Ok(Action::Override),
        "resolve" => Ok(Action::Resolve),
        "promote" => Ok(Action::Promote),
        "milestone" => Ok(Action::Milestone),
        "admin" => Ok(Action::Admin),
        _ => anyhow::bail!(
            "unknown action: {} (expected: read, write, create, delete, move, override, resolve, promote, milestone, admin)",
            s
        ),
    }
}

fn parse_resource(s: &str) -> Result<Resource> {
    // Simple parsing - use ResourcePattern for the heavy lifting
    let pattern = conflux_rbac::ResourcePattern::parse(s)?;

    Ok(Resource {
        entity_type: pattern.entity_type,
        entity_id: pattern.entity_id,
        field: pattern.field,
        environment: pattern.environment,
    })
}
