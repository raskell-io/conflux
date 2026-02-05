//! Multi-actor concurrency tests.
//!
//! Tests that concurrent operations from multiple actors merge correctly
//! according to the schema-defined merge strategies.

use conflux_core::{ActorClass, ActorId, ApplyResult, Clock, Document, EntityId, FieldValue, Operation};
use conflux_schema::Schema;
use std::str::FromStr;

fn test_schema() -> Schema {
    let toml = r#"
        [schema]
        name = "multi-actor-test"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "replicas", type = "int", merge = "max" },
            { name = "image", type = "string", merge = "lww" },
            { name = "min_replicas", type = "int", merge = "min" },
        ]

        [entity.config]
        fields = [
            { name = "value", type = "string", merge = "review" },
        ]
    "#;
    Schema::from_str(toml).expect("valid schema")
}

#[test]
fn concurrent_max_merge_takes_highest() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();

    let human = ActorId::new("operator", ActorClass::Human);
    let autoscaler = ActorId::new("autoscaler", ActorClass::Operator);

    let mut doc = Document::new();

    // Create entity
    let create_op = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &human,
        clock.new_timestamp(),
    );
    doc.apply(&create_op, &schema_info, &clock).unwrap();

    // Human sets replicas to 3
    let human_op = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(3),
        &human,
        clock.new_timestamp(),
    );
    doc.apply(&human_op, &schema_info, &clock).unwrap();

    // Autoscaler sets replicas to 5 (concurrent, simulated by applying after)
    let autoscaler_op = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(5),
        &autoscaler,
        clock.new_timestamp(),
    );
    doc.apply(&autoscaler_op, &schema_info, &clock).unwrap();

    // Max wins: should be 5
    let replicas = doc.get_field(&EntityId::new("service.api"), "replicas").unwrap();
    assert_eq!(replicas, FieldValue::Int(5));
}

#[test]
fn concurrent_min_merge_takes_lowest() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();

    let alice = ActorId::new("alice", ActorClass::Human);
    let bob = ActorId::new("bob", ActorClass::Human);

    let mut doc = Document::new();

    // Create entity
    let create_op = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&create_op, &schema_info, &clock).unwrap();

    // Alice sets min_replicas to 5
    let alice_op = Operation::set_field(
        "service.api",
        "min_replicas",
        FieldValue::Int(5),
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&alice_op, &schema_info, &clock).unwrap();

    // Bob sets min_replicas to 2 (lower)
    let bob_op = Operation::set_field(
        "service.api",
        "min_replicas",
        FieldValue::Int(2),
        &bob,
        clock.new_timestamp(),
    );
    doc.apply(&bob_op, &schema_info, &clock).unwrap();

    // Min wins: should be 2
    let min_replicas = doc.get_field(&EntityId::new("service.api"), "min_replicas").unwrap();
    assert_eq!(min_replicas, FieldValue::Int(2));
}

#[test]
fn concurrent_lww_merge_takes_later_timestamp() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();

    let alice = ActorId::new("alice", ActorClass::Human);
    let pipeline = ActorId::new("deploy-pipeline", ActorClass::Pipeline);

    let mut doc = Document::new();

    // Create entity
    let create_op = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&create_op, &schema_info, &clock).unwrap();

    // Alice sets image to v1.0
    let alice_op = Operation::set_field(
        "service.api",
        "image",
        FieldValue::String("myapp:v1.0".into()),
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&alice_op, &schema_info, &clock).unwrap();

    // Pipeline sets image to v2.0 (later timestamp wins)
    let pipeline_op = Operation::set_field(
        "service.api",
        "image",
        FieldValue::String("myapp:v2.0".into()),
        &pipeline,
        clock.new_timestamp(),
    );
    doc.apply(&pipeline_op, &schema_info, &clock).unwrap();

    // LWW: later timestamp (pipeline) wins
    let image = doc.get_field(&EntityId::new("service.api"), "image").unwrap();
    assert_eq!(image, FieldValue::String("myapp:v2.0".into()));
}

#[test]
fn concurrent_review_merge_flags_conflict() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();

    let alice = ActorId::new("alice", ActorClass::Human);
    let bob = ActorId::new("bob", ActorClass::Human);

    let mut doc = Document::new();

    // Create entity
    let create_op = Operation::insert_entity(
        "config.critical",
        "config",
        None,
        None,
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&create_op, &schema_info, &clock).unwrap();

    // Alice sets value
    let alice_op = Operation::set_field(
        "config.critical",
        "value",
        FieldValue::String("alice-value".into()),
        &alice,
        clock.new_timestamp(),
    );
    let result1 = doc.apply(&alice_op, &schema_info, &clock).unwrap();
    assert!(matches!(result1, ApplyResult::Applied));

    // Bob sets different value (concurrent)
    let bob_op = Operation::set_field(
        "config.critical",
        "value",
        FieldValue::String("bob-value".into()),
        &bob,
        clock.new_timestamp(),
    );
    let result2 = doc.apply(&bob_op, &schema_info, &clock).unwrap();

    // Review strategy should flag conflict
    assert!(matches!(result2, ApplyResult::Conflict(_)));
}

#[test]
fn different_fields_never_conflict() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();

    let alice = ActorId::new("alice", ActorClass::Human);
    let bob = ActorId::new("bob", ActorClass::Human);

    let mut doc = Document::new();

    // Create entity
    let create_op = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&create_op, &schema_info, &clock).unwrap();

    // Alice sets replicas
    let alice_op = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(5),
        &alice,
        clock.new_timestamp(),
    );
    let result1 = doc.apply(&alice_op, &schema_info, &clock).unwrap();
    assert!(matches!(result1, ApplyResult::Applied));

    // Bob sets image (different field, no conflict possible)
    let bob_op = Operation::set_field(
        "service.api",
        "image",
        FieldValue::String("myapp:v2.0".into()),
        &bob,
        clock.new_timestamp(),
    );
    let result2 = doc.apply(&bob_op, &schema_info, &clock).unwrap();
    assert!(matches!(result2, ApplyResult::Applied));

    // Both values should be set
    let replicas = doc.get_field(&EntityId::new("service.api"), "replicas").unwrap();
    let image = doc.get_field(&EntityId::new("service.api"), "image").unwrap();
    assert_eq!(replicas, FieldValue::Int(5));
    assert_eq!(image, FieldValue::String("myapp:v2.0".into()));
}

#[test]
fn different_entities_never_conflict() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();

    let alice = ActorId::new("alice", ActorClass::Human);
    let bob = ActorId::new("bob", ActorClass::Human);

    let mut doc = Document::new();

    // Create two entities
    let create_op1 = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &alice,
        clock.new_timestamp(),
    );
    doc.apply(&create_op1, &schema_info, &clock).unwrap();

    let create_op2 = Operation::insert_entity(
        "service.worker",
        "service",
        None,
        None,
        &bob,
        clock.new_timestamp(),
    );
    doc.apply(&create_op2, &schema_info, &clock).unwrap();

    // Alice modifies service.api
    let alice_op = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(5),
        &alice,
        clock.new_timestamp(),
    );
    let result1 = doc.apply(&alice_op, &schema_info, &clock).unwrap();
    assert!(matches!(result1, ApplyResult::Applied));

    // Bob modifies service.worker (different entity, no conflict possible)
    let bob_op = Operation::set_field(
        "service.worker",
        "replicas",
        FieldValue::Int(10),
        &bob,
        clock.new_timestamp(),
    );
    let result2 = doc.apply(&bob_op, &schema_info, &clock).unwrap();
    assert!(matches!(result2, ApplyResult::Applied));

    // Both entities should have their values
    let api_replicas = doc.get_field(&EntityId::new("service.api"), "replicas").unwrap();
    let worker_replicas = doc.get_field(&EntityId::new("service.worker"), "replicas").unwrap();
    assert_eq!(api_replicas, FieldValue::Int(5));
    assert_eq!(worker_replicas, FieldValue::Int(10));
}
