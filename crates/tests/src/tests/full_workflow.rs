//! Full workflow integration tests.
//!
//! Tests the complete lifecycle: schema loading → document creation →
//! operations → persistence → milestone projection.

use conflux_core::{ActorClass, ActorId, Clock, Document, EntityId, FieldValue, Operation};
use conflux_git::{MilestoneProjector, OutputFormat, ProjectorConfig};
use conflux_schema::Schema;
use conflux_store::{OperationQuery, SqliteStore};
use std::str::FromStr;
use tempfile::TempDir;

fn test_schema() -> Schema {
    let toml = r#"
        [schema]
        name = "test-infrastructure"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "image", type = "string", merge = "lww" },
            { name = "replicas", type = "int", merge = "max" },
            { name = "enabled", type = "bool", merge = "lww" },
        ]

        [entity.route]
        fields = [
            { name = "path", type = "string", merge = "lww" },
            { name = "weight", type = "int", merge = "max" },
            { name = "timeout", type = "duration", merge = "min" },
        ]
    "#;
    Schema::from_str(toml).expect("valid schema")
}

fn test_actor() -> ActorId {
    ActorId::new("test-user", ActorClass::Human)
}

#[test]
fn full_workflow_create_entities_set_fields_persist() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();
    let store = SqliteStore::open_in_memory().expect("open store");

    let mut document = Document::new();
    let doc_id = "test-doc";

    // 1. Create a service entity
    let op1 = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &actor,
        clock.new_timestamp(),
    );
    document.apply(&op1, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op1).unwrap();

    // 2. Set fields on the service
    let op2 = Operation::set_field(
        "service.api",
        "image",
        FieldValue::String("myapp:v1.0".into()),
        &actor,
        clock.new_timestamp(),
    );
    document.apply(&op2, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op2).unwrap();

    let op3 = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(3),
        &actor,
        clock.new_timestamp(),
    );
    document.apply(&op3, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op3).unwrap();

    // 3. Create a route entity
    let op4 = Operation::insert_entity(
        "route.api-public",
        "route",
        None,
        None,
        &actor,
        clock.new_timestamp(),
    );
    document.apply(&op4, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op4).unwrap();

    let op5 = Operation::set_field(
        "route.api-public",
        "path",
        FieldValue::String("/api/*".into()),
        &actor,
        clock.new_timestamp(),
    );
    document.apply(&op5, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op5).unwrap();

    // 4. Verify document state
    let entity = document.get_entity(&EntityId::new("service.api")).unwrap();
    assert_eq!(entity.entity_type, "service");

    let image = document.get_field(&EntityId::new("service.api"), "image").unwrap();
    assert_eq!(image, FieldValue::String("myapp:v1.0".into()));

    let replicas = document.get_field(&EntityId::new("service.api"), "replicas").unwrap();
    assert_eq!(replicas, FieldValue::Int(3));

    // 5. Verify persistence
    let ops = store
        .query_operations(&OperationQuery::new(doc_id))
        .unwrap();
    assert_eq!(ops.len(), 5);

    // 6. Verify we can rebuild document from stored operations
    let mut rebuilt_doc = Document::new();
    for stored in &ops {
        rebuilt_doc
            .apply(&stored.operation, &schema_info, &clock)
            .unwrap();
    }

    let rebuilt_image = rebuilt_doc.get_field(&EntityId::new("service.api"), "image").unwrap();
    assert_eq!(rebuilt_image, image);
}

#[test]
fn workflow_with_snapshots() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();
    let store = SqliteStore::open_in_memory().expect("open store");

    let mut document = Document::new();
    let doc_id = "snapshot-test";

    // Create some operations
    for i in 0..10 {
        let op = Operation::insert_entity(
            format!("service.svc-{}", i),
            "service",
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        document.apply(&op, &schema_info, &clock).unwrap();
        store.append_operation(doc_id, &op).unwrap();
    }

    // Save a snapshot
    let snapshot_ts = clock.new_timestamp();
    store
        .save_snapshot(doc_id, &snapshot_ts, &document)
        .unwrap();

    // Add more operations after snapshot
    for i in 10..15 {
        let op = Operation::insert_entity(
            format!("service.svc-{}", i),
            "service",
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        document.apply(&op, &schema_info, &clock).unwrap();
        store.append_operation(doc_id, &op).unwrap();
    }

    // Verify operations since snapshot
    let ops_since = store.operations_since_snapshot(doc_id).unwrap();
    assert_eq!(ops_since, 5);

    // Load snapshot and replay
    let snapshot = store.latest_snapshot(doc_id).unwrap();
    assert_eq!(snapshot.document.entities.len(), 10);
}

#[test]
fn workflow_with_milestones() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();
    let store = SqliteStore::open_in_memory().expect("open store");
    let temp_dir = TempDir::new().expect("create temp dir");

    // Initialize a git repository in the temp directory
    std::process::Command::new("git")
        .current_dir(temp_dir.path())
        .args(["init"])
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .current_dir(temp_dir.path())
        .args(["config", "user.email", "test@example.com"])
        .output()
        .expect("git config email");
    std::process::Command::new("git")
        .current_dir(temp_dir.path())
        .args(["config", "user.name", "Test"])
        .output()
        .expect("git config name");

    let mut document = Document::new();
    let doc_id = "milestone-test";

    // Create some operations
    let mut ops = Vec::new();
    for i in 0..5 {
        let op = Operation::insert_entity(
            format!("service.svc-{}", i),
            "service",
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        document.apply(&op, &schema_info, &clock).unwrap();
        store.append_operation(doc_id, &op).unwrap();
        ops.push(op);
    }

    // Configure git projector
    let config = ProjectorConfig {
        repo_path: temp_dir.path().to_path_buf(),
        default_format: OutputFormat::Yaml,
        author_name: "Test".to_string(),
        author_email: "test@example.com".to_string(),
    };
    let projector = MilestoneProjector::new(config);

    // Project to git
    let result = projector
        .project(&document, ops.clone(), &["production".to_string()], "Initial setup")
        .expect("project milestone");

    assert!(!result.commit_sha.is_empty());
    assert_eq!(result.operation_count, 5);

    // Record milestone
    let milestone_id = store
        .record_milestone(
            doc_id,
            Some(&result.commit_sha),
            &result.hlc_start.unwrap(),
            &result.hlc_end.unwrap(),
            Some("Initial setup"),
        )
        .unwrap();

    // Verify milestone
    let milestones = store.list_milestones(doc_id).unwrap();
    assert_eq!(milestones.len(), 1);
    assert_eq!(milestones[0].id, milestone_id);
}

#[test]
fn workflow_query_operations_filters() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let store = SqliteStore::open_in_memory().expect("open store");

    let mut document = Document::new();
    let doc_id = "query-test";

    let alice = ActorId::new("alice", ActorClass::Human);
    let bob = ActorId::new("bob", ActorClass::Human);
    let autoscaler = ActorId::new("autoscaler", ActorClass::Operator);

    // Alice creates service
    let op1 = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &alice,
        clock.new_timestamp(),
    );
    document.apply(&op1, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op1).unwrap();

    // Bob creates route
    let op2 = Operation::insert_entity(
        "route.main",
        "route",
        None,
        None,
        &bob,
        clock.new_timestamp(),
    );
    document.apply(&op2, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op2).unwrap();

    // Autoscaler sets replicas
    let op3 = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(5),
        &autoscaler,
        clock.new_timestamp(),
    );
    document.apply(&op3, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op3).unwrap();

    // Alice sets image
    let op4 = Operation::set_field(
        "service.api",
        "image",
        FieldValue::String("v2.0".into()),
        &alice,
        clock.new_timestamp(),
    );
    document.apply(&op4, &schema_info, &clock).unwrap();
    store.append_operation(doc_id, &op4).unwrap();

    // Query by entity
    let service_ops = store
        .query_operations(&OperationQuery::new(doc_id).for_entity("service.api"))
        .unwrap();
    assert_eq!(service_ops.len(), 3); // insert + 2 set_field

    // Query by actor
    let alice_ops = store
        .query_operations(&OperationQuery::new(doc_id).by_actor("alice"))
        .unwrap();
    assert_eq!(alice_ops.len(), 2);

    // Query with limit
    let limited = store
        .query_operations(&OperationQuery::new(doc_id).limit(2))
        .unwrap();
    assert_eq!(limited.len(), 2);
}
