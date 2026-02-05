//! Schema validation integration tests.
//!
//! Tests that schema validation correctly enforces constraints
//! on operations and document state.

use conflux_core::{ActorClass, ActorId, Clock, Document, FieldValue, Operation};
use conflux_schema::Schema;
use std::str::FromStr;

fn test_actor() -> ActorId {
    ActorId::new("test", ActorClass::Human)
}

#[test]
fn valid_schema_parses_successfully() {
    let toml = r#"
        [schema]
        name = "valid-schema"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "image", type = "string", merge = "lww" },
            { name = "replicas", type = "int", merge = "max" },
        ]

        [entity.route]
        fields = [
            { name = "path", type = "string", merge = "lww" },
        ]
    "#;

    let schema = Schema::from_str(toml);
    assert!(schema.is_ok());

    let schema = schema.unwrap();
    assert_eq!(schema.name, "valid-schema");
    assert_eq!(schema.version, "1.0.0");
    assert!(schema.entities.contains_key("service"));
    assert!(schema.entities.contains_key("route"));
}

#[test]
fn schema_with_all_field_types() {
    let toml = r#"
        [schema]
        name = "all-types"
        version = "1.0.0"

        [entity.example]
        fields = [
            { name = "str_field", type = "string", merge = "lww" },
            { name = "int_field", type = "int", merge = "max" },
            { name = "float_field", type = "float", merge = "lww" },
            { name = "bool_field", type = "bool", merge = "lww" },
            { name = "duration_field", type = "duration", merge = "min" },
            { name = "map_field", type = "map", merge = "lww" },
        ]
    "#;

    let schema = Schema::from_str(toml);
    assert!(schema.is_ok());

    let schema = schema.unwrap();
    let entity = &schema.entities["example"];
    assert_eq!(entity.fields.len(), 6);
}

#[test]
fn schema_with_all_merge_strategies() {
    let toml = r#"
        [schema]
        name = "all-merges"
        version = "1.0.0"

        [entity.example]
        fields = [
            { name = "lww_field", type = "string", merge = "lww" },
            { name = "max_field", type = "int", merge = "max" },
            { name = "min_field", type = "int", merge = "min" },
            { name = "review_field", type = "string", merge = "review" },
        ]
    "#;

    let schema = Schema::from_str(toml);
    assert!(schema.is_ok());
}

#[test]
fn schema_with_environments() {
    let toml = r#"
[schema]
name = "env-test"
version = "1.0.0"

[environments]
base = "production"

[environments.overlays]
staging = { inherits = "production" }
development = { inherits = "staging" }

[entity.service]
fields = [
    { name = "replicas", type = "int", merge = "max" },
]
    "#;

    let schema = Schema::from_str(toml);
    assert!(schema.is_ok());

    let schema = schema.unwrap();
    let envs = schema.environments.as_ref().unwrap();
    assert_eq!(envs.base, "production");
    assert_eq!(envs.overlays.len(), 2);
}

#[test]
fn operations_validated_against_schema() {
    let toml = r#"
        [schema]
        name = "validation-test"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "replicas", type = "int", merge = "max" },
            { name = "image", type = "string", merge = "lww" },
        ]
    "#;

    let schema = Schema::from_str(toml).unwrap();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();

    let mut doc = Document::new();

    // Create entity with valid type
    let op = Operation::insert_entity(
        "service.api",
        "service",
        None,
        None,
        &actor,
        clock.new_timestamp(),
    );
    let result = doc.apply(&op, &schema_info, &clock);
    assert!(result.is_ok());

    // Set field with correct type (int for replicas)
    let op = Operation::set_field(
        "service.api",
        "replicas",
        FieldValue::Int(5),
        &actor,
        clock.new_timestamp(),
    );
    let result = doc.apply(&op, &schema_info, &clock);
    assert!(result.is_ok());

    // Set field with correct type (string for image)
    let op = Operation::set_field(
        "service.api",
        "image",
        FieldValue::String("myapp:v1.0".into()),
        &actor,
        clock.new_timestamp(),
    );
    let result = doc.apply(&op, &schema_info, &clock);
    assert!(result.is_ok());
}

#[test]
fn schema_provides_merge_strategy_via_field_def() {
    use conflux_core::MergeStrategy;

    let toml = r#"
        [schema]
        name = "merge-strategy-test"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "replicas", type = "int", merge = "max" },
            { name = "image", type = "string", merge = "lww" },
            { name = "min_instances", type = "int", merge = "min" },
        ]
    "#;

    let schema = Schema::from_str(toml).unwrap();
    let service = &schema.entities["service"];

    assert_eq!(
        service.fields["replicas"].merge_strategy,
        MergeStrategy::Max
    );
    assert_eq!(
        service.fields["image"].merge_strategy,
        MergeStrategy::Lww
    );
    assert_eq!(
        service.fields["min_instances"].merge_strategy,
        MergeStrategy::Min
    );
}

#[test]
fn entity_types_must_exist_in_schema() {
    let toml = r#"
        [schema]
        name = "entity-type-test"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "image", type = "string", merge = "lww" },
        ]
    "#;

    let schema = Schema::from_str(toml).unwrap();

    // service exists
    assert!(schema.entities.contains_key("service"));

    // route does not exist
    assert!(!schema.entities.contains_key("route"));
}

#[test]
fn field_constraints_in_schema() {
    let toml = r#"
        [schema]
        name = "constraint-test"
        version = "1.0.0"

        [entity.service]
        fields = [
            { name = "replicas", type = "int", merge = "max", required = true },
            { name = "enabled", type = "bool", merge = "lww" },
        ]
    "#;

    let schema = Schema::from_str(toml).unwrap();

    let service = &schema.entities["service"];
    let replicas = &service.fields["replicas"];
    let enabled = &service.fields["enabled"];

    assert!(replicas.required);
    assert!(!enabled.required);
}
