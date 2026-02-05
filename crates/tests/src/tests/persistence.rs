//! Persistence integration tests.
//!
//! Tests that documents can be persisted and recovered correctly,
//! including snapshot-based recovery.

use conflux_core::{ActorClass, ActorId, Clock, Document, EntityId, FieldValue, Operation};
use conflux_schema::Schema;
use conflux_store::{OperationQuery, SqliteStore};
use std::str::FromStr;
use tempfile::TempDir;

fn test_schema() -> Schema {
    let toml = r#"
        [schema]
        name = "persistence-test"
        version = "1.0.0"

        [entity.item]
        fields = [
            { name = "name", type = "string", merge = "lww" },
            { name = "count", type = "int", merge = "max" },
        ]
    "#;
    Schema::from_str(toml).expect("valid schema")
}

fn test_actor() -> ActorId {
    ActorId::new("test", ActorClass::Human)
}

#[test]
fn persist_and_recover_document_from_operations() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();

    let temp_dir = TempDir::new().expect("create temp dir");
    let db_path = temp_dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap();

    let doc_id = "persist-test";

    // Phase 1: Create document and persist
    {
        let store = SqliteStore::open(db_path_str).expect("open store");
        let mut doc = Document::new();

        for i in 0..10 {
            let op = Operation::insert_entity(
                format!("item.{}", i),
                "item",
                None,
                None,
                &actor,
                clock.new_timestamp(),
            );
            doc.apply(&op, &schema_info, &clock).unwrap();
            store.append_operation(doc_id, &op).unwrap();

            let set_op = Operation::set_field(
                format!("item.{}", i),
                "name",
                FieldValue::String(format!("Item {}", i)),
                &actor,
                clock.new_timestamp(),
            );
            doc.apply(&set_op, &schema_info, &clock).unwrap();
            store.append_operation(doc_id, &set_op).unwrap();
        }

        assert_eq!(doc.entities.len(), 10);
    }

    // Phase 2: Reopen and recover
    {
        let store = SqliteStore::open(db_path_str).expect("reopen store");
        let mut recovered_doc = Document::new();

        let ops = store
            .query_operations(&OperationQuery::new(doc_id))
            .expect("query ops");
        assert_eq!(ops.len(), 20); // 10 inserts + 10 sets

        for stored in ops {
            recovered_doc
                .apply(&stored.operation, &schema_info, &clock)
                .unwrap();
        }

        assert_eq!(recovered_doc.entities.len(), 10);

        let name = recovered_doc.get_field(&EntityId::new("item.5"), "name").unwrap();
        assert_eq!(name, FieldValue::String("Item 5".into()));
    }
}

#[test]
fn multiple_documents_isolated() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();

    let store = SqliteStore::open_in_memory().expect("open store");

    let mut doc_a = Document::new();
    let mut doc_b = Document::new();

    // Add to doc A
    for i in 0..5 {
        let op = Operation::insert_entity(
            format!("item.a{}", i),
            "item",
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        doc_a.apply(&op, &schema_info, &clock).unwrap();
        store.append_operation("doc-a", &op).unwrap();
    }

    // Add to doc B
    for i in 0..3 {
        let op = Operation::insert_entity(
            format!("item.b{}", i),
            "item",
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        doc_b.apply(&op, &schema_info, &clock).unwrap();
        store.append_operation("doc-b", &op).unwrap();
    }

    // Verify isolation
    let ops_a = store
        .query_operations(&OperationQuery::new("doc-a"))
        .unwrap();
    let ops_b = store
        .query_operations(&OperationQuery::new("doc-b"))
        .unwrap();

    assert_eq!(ops_a.len(), 5);
    assert_eq!(ops_b.len(), 3);

    assert_eq!(store.operation_count("doc-a").unwrap(), 5);
    assert_eq!(store.operation_count("doc-b").unwrap(), 3);
}

#[test]
fn operations_ordered_by_hlc_timestamp() {
    let schema = test_schema();
    let schema_info = schema.as_schema_info();
    let clock = Clock::new();
    let actor = test_actor();

    let store = SqliteStore::open_in_memory().expect("open store");
    let mut doc = Document::new();
    let doc_id = "order-test";

    // Create operations with sequential timestamps
    for i in 0..10 {
        let op = Operation::insert_entity(
            format!("item.{}", i),
            "item",
            None,
            None,
            &actor,
            clock.new_timestamp(),
        );
        doc.apply(&op, &schema_info, &clock).unwrap();
        store.append_operation(doc_id, &op).unwrap();
    }

    // Query and verify order
    let ops = store
        .query_operations(&OperationQuery::new(doc_id))
        .unwrap();

    for i in 1..ops.len() {
        let prev_ts = &ops[i - 1].operation.timestamp;
        let curr_ts = &ops[i].operation.timestamp;
        assert!(prev_ts < curr_ts, "Operations should be ordered by timestamp");
    }
}
