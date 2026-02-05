//! Integration tests for multi-node replication.

use conflux_core::{ActorClass, ActorId, Clock, FieldValue, Operation};
use conflux_replication::{
    ClusterConfig, ClusterRole, ClusterState, NodeId, VersionVector,
};
use conflux_store::SqliteStore;

fn test_actor(name: &str) -> ActorId {
    ActorId::new(name, ActorClass::Operator)
}

fn make_set_field_op(
    clock: &Clock,
    actor: &ActorId,
    entity_id: &str,
    field: &str,
    value: i64,
) -> Operation {
    Operation::set_field(
        entity_id,
        field,
        FieldValue::Int(value),
        actor,
        clock.new_timestamp(),
    )
}

#[test]
fn version_vector_tracks_operations_from_multiple_nodes() {
    let clock_a = Clock::with_node_id("node-a").unwrap();
    let clock_b = Clock::with_node_id("node-b").unwrap();

    let mut vv = VersionVector::new();

    // Operations from node-a
    let actor_a = test_actor("node-a");
    let op1 = make_set_field_op(&clock_a, &actor_a, "entity.1", "field1", 1);
    let op2 = make_set_field_op(&clock_a, &actor_a, "entity.1", "field1", 2);

    // Operations from node-b
    let actor_b = test_actor("node-b");
    let op3 = make_set_field_op(&clock_b, &actor_b, "entity.1", "field2", 10);

    // Update version vector
    vv.update(&NodeId::new("node-a"), op1.timestamp);
    vv.update(&NodeId::new("node-a"), op2.timestamp);
    vv.update(&NodeId::new("node-b"), op3.timestamp);

    // Check that highest timestamps are tracked
    assert_eq!(vv.get(&NodeId::new("node-a")), Some(&op2.timestamp));
    assert_eq!(vv.get(&NodeId::new("node-b")), Some(&op3.timestamp));
}

#[test]
fn version_vector_diff_identifies_missing_operations() {
    let clock = Clock::new();

    let ts1 = clock.new_timestamp();
    let ts2 = clock.new_timestamp();
    let ts3 = clock.new_timestamp();

    // Node A's version vector: knows about ops from both nodes
    let mut vv_a = VersionVector::new();
    vv_a.update(&NodeId::new("node-a"), ts3);
    vv_a.update(&NodeId::new("node-b"), ts2);

    // Node B's version vector: only knows about its own ops, not all of node-a's
    let mut vv_b = VersionVector::new();
    vv_b.update(&NodeId::new("node-a"), ts1); // Older timestamp
    vv_b.update(&NodeId::new("node-b"), ts2);

    // Diff shows what B is missing from A
    let diff = vv_a.diff(&vv_b);

    // B is missing newer ops from node-a (ts3 > ts1)
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].0, NodeId::new("node-a"));
    assert_eq!(diff[0].1, Some(ts1)); // B's last known timestamp for node-a
}

#[test]
fn cluster_state_leader_election_three_nodes() {
    // 3-node cluster: node-a, node-b, node-c
    // Quorum = 2

    let config = ClusterConfig::new("node-b")
        .with_peer("node-a", "a:9401")
        .with_peer("node-c", "c:9401");

    let state = ClusterState::new(&config);

    // Initially unknown, no peers connected (1 reachable, need 2)
    assert_eq!(state.role(), ClusterRole::Unknown);

    // Connect node-c (highest ID)
    state.peer_connected(&NodeId::new("node-c"), ClusterRole::Unknown);

    // 2/3 reachable, quorum met
    // node-c has highest ID, so we should be follower
    let role = state.compute_role();
    assert_eq!(role, ClusterRole::Follower);

    // Now connect node-a instead
    state.peer_disconnected(&NodeId::new("node-c"));
    state.peer_connected(&NodeId::new("node-a"), ClusterRole::Unknown);

    // node-b is highest among (node-a, node-b), so we're leader
    let role = state.compute_role();
    assert_eq!(role, ClusterRole::Leader);
}

#[test]
fn cluster_state_loses_quorum() {
    let config = ClusterConfig::new("node-a")
        .with_peer("node-b", "b:9401")
        .with_peer("node-c", "c:9401");

    let state = ClusterState::new(&config);

    // Connect both peers
    state.peer_connected(&NodeId::new("node-b"), ClusterRole::Unknown);
    state.peer_connected(&NodeId::new("node-c"), ClusterRole::Unknown);

    // All 3 reachable, node-c is highest -> follower
    let role = state.compute_role();
    assert_eq!(role, ClusterRole::Follower);

    // Lose both peers
    state.peer_disconnected(&NodeId::new("node-b"));
    state.peer_disconnected(&NodeId::new("node-c"));

    // Only 1/3 reachable -> no quorum -> follower
    let role = state.compute_role();
    assert_eq!(role, ClusterRole::Follower);
}

#[test]
fn store_operations_by_actor_for_sync() {
    let store = SqliteStore::open_in_memory().unwrap();
    let clock = Clock::new();

    let actor_a = test_actor("node-a");
    let actor_b = test_actor("node-b");

    // Create operations from different nodes
    let op1 = make_set_field_op(&clock, &actor_a, "entity.1", "field", 1);
    let ts_after_1 = clock.new_timestamp();
    let op2 = make_set_field_op(&clock, &actor_b, "entity.1", "field", 2);
    let op3 = make_set_field_op(&clock, &actor_a, "entity.1", "field", 3);

    store.append_operation("doc-1", &op1).unwrap();
    store.append_operation("doc-1", &op2).unwrap();
    store.append_operation("doc-1", &op3).unwrap();

    // Query operations from node-a since ts_after_1
    let ops = store
        .get_operations_by_actor_since("doc-1", "node-a", Some(&ts_after_1))
        .unwrap();

    // Should only get op3 (op1 was before ts_after_1)
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].operation.id, op3.id);
}

#[test]
fn idempotent_operation_handling() {
    let store = SqliteStore::open_in_memory().unwrap();
    let clock = Clock::new();
    let actor = test_actor("node-a");

    let op = make_set_field_op(&clock, &actor, "entity.1", "field", 42);

    // First insert succeeds
    assert!(!store.operation_exists(&op.id).unwrap());
    store.append_operation("doc-1", &op).unwrap();
    assert!(store.operation_exists(&op.id).unwrap());

    // Trying to insert again should fail (PRIMARY KEY constraint)
    let result = store.append_operation("doc-1", &op);
    assert!(result.is_err());
}

#[test]
fn version_vector_persistence() {
    let store = SqliteStore::open_in_memory().unwrap();
    let clock = Clock::new();

    let mut vv = VersionVector::new();
    vv.update(&NodeId::new("node-a"), clock.new_timestamp());
    vv.update(&NodeId::new("node-b"), clock.new_timestamp());

    // Save version vector
    let vv_json = serde_json::to_string(&vv).unwrap();
    store.save_version_vector("doc-1", &vv_json).unwrap();

    // Load and verify
    let loaded_json = store.load_version_vector("doc-1").unwrap();
    let loaded_vv: VersionVector = serde_json::from_str(&loaded_json).unwrap();

    assert_eq!(vv, loaded_vv);
}

#[test]
fn operations_converge_regardless_of_order() {
    // This tests CRDT commutativity through the replication lens
    use conflux_core::{Document, EntityId};
    use conflux_schema::Schema;
    use std::io::Write;
    use tempfile::NamedTempFile;

    let schema_toml = r#"
[schema]
name = "test"
version = "1.0"

[entity.item]
fields = [
    { name = "count", type = "int", merge = "max" }
]
"#;

    // Write schema to temp file
    let mut temp = NamedTempFile::new().unwrap();
    temp.write_all(schema_toml.as_bytes()).unwrap();

    let schema = Schema::from_file(temp.path()).unwrap();
    let schema_info = schema.as_schema_info();

    // Two nodes receive operations in different orders
    let clock_a = Clock::with_node_id("node-a").unwrap();
    let clock_b = Clock::with_node_id("node-b").unwrap();

    let actor_a = test_actor("node-a");
    let actor_b = test_actor("node-b");

    // Create entity on both nodes (using same ID)
    let insert_a = Operation::insert_entity(
        "item.x",
        "item",
        None,
        None,
        &actor_a,
        clock_a.new_timestamp(),
    );

    // Set count to 10 on node-a
    let set_a = make_set_field_op(&clock_a, &actor_a, "item.x", "count", 10);

    // Set count to 5 on node-b (concurrently)
    let set_b = make_set_field_op(&clock_b, &actor_b, "item.x", "count", 5);

    // Node A applies: insert_a, set_a, set_b
    let mut doc_a = Document::new();
    doc_a.apply(&insert_a, &schema_info, &clock_a).unwrap();
    doc_a.apply(&set_a, &schema_info, &clock_a).unwrap();
    doc_a.apply(&set_b, &schema_info, &clock_a).unwrap();

    // Node B applies: insert_a, set_b, set_a (different order)
    let mut doc_b = Document::new();
    doc_b.apply(&insert_a, &schema_info, &clock_b).unwrap();
    doc_b.apply(&set_b, &schema_info, &clock_b).unwrap();
    doc_b.apply(&set_a, &schema_info, &clock_b).unwrap();

    // Both should converge to the same value (max wins, so 10)
    let entity_id = EntityId::new("item.x");
    let value_a = doc_a.get_field(&entity_id, "count").unwrap();
    let value_b = doc_b.get_field(&entity_id, "count").unwrap();

    assert_eq!(value_a, value_b);
    assert_eq!(value_a, FieldValue::Int(10));
}
