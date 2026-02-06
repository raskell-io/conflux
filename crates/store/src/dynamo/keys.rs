//! Key builders for DynamoDB single-table design.

use conflux_core::HlcTimestamp;
use uuid::Uuid;

/// Key prefix constants and builders for the single-table design.
pub struct Keys;

impl Keys {
    // --- Partition Key Builders ---

    /// Builds the partition key for a document: `DOC#<document_id>`
    pub fn doc_pk(document_id: &str) -> String {
        format!("DOC#{document_id}")
    }

    /// Builds the GSI1 partition key for operation lookup: `OP#<operation_id>`
    pub fn operation_gsi1_pk(operation_id: &Uuid) -> String {
        format!("OP#{operation_id}")
    }

    /// Builds the GSI2 partition key for actor-based queries: `ACTOR#<document_id>#<actor_id>`
    pub fn actor_gsi2_pk(document_id: &str, actor_id: &str) -> String {
        format!("ACTOR#{document_id}#{actor_id}")
    }

    // --- Sort Key Builders ---

    /// Builds the sort key for an operation: `OP#<hlc_timestamp>#<operation_id>`
    ///
    /// Using HLC as the primary sort component ensures causal ordering.
    pub fn operation_sk(hlc: &HlcTimestamp, operation_id: &Uuid) -> String {
        format!("OP#{}#{}", hlc, operation_id)
    }

    /// Builds the sort key for a snapshot: `SNAP#<hlc_timestamp>#<snapshot_id>`
    pub fn snapshot_sk(hlc: &HlcTimestamp, snapshot_id: &Uuid) -> String {
        format!("SNAP#{}#{}", hlc, snapshot_id)
    }

    /// Builds the sort key for a milestone: `MILE#<created_at>#<milestone_id>`
    ///
    /// Using created_at as the primary sort component enables newest-first queries.
    pub fn milestone_sk(created_at: &str, milestone_id: &Uuid) -> String {
        format!("MILE#{}#{}", created_at, milestone_id)
    }

    /// The sort key for a version vector: `VV#`
    pub fn version_vector_sk() -> &'static str {
        "VV#"
    }

    // --- Sort Key Prefixes (for range queries) ---

    /// Prefix for operation sort keys.
    pub const OP_PREFIX: &'static str = "OP#";

    /// Prefix for snapshot sort keys.
    pub const SNAP_PREFIX: &'static str = "SNAP#";

    /// Prefix for milestone sort keys.
    pub const MILE_PREFIX: &'static str = "MILE#";

    // --- Parsing ---

    /// Extracts the HLC timestamp from an operation sort key.
    ///
    /// Input format: `OP#<hlc>#<uuid>`
    #[allow(dead_code)]
    pub fn parse_operation_sk_hlc(sk: &str) -> Option<&str> {
        let rest = sk.strip_prefix(Self::OP_PREFIX)?;
        let parts: Vec<&str> = rest.splitn(2, '#').collect();
        parts.first().copied()
    }

    /// Extracts the operation ID from an operation sort key.
    ///
    /// Input format: `OP#<hlc>#<uuid>`
    #[allow(dead_code)]
    pub fn parse_operation_sk_id(sk: &str) -> Option<&str> {
        let rest = sk.strip_prefix(Self::OP_PREFIX)?;
        let parts: Vec<&str> = rest.splitn(2, '#').collect();
        parts.get(1).copied()
    }

    /// Extracts the HLC timestamp from a snapshot sort key.
    ///
    /// Input format: `SNAP#<hlc>#<uuid>`
    #[allow(dead_code)]
    pub fn parse_snapshot_sk_hlc(sk: &str) -> Option<&str> {
        let rest = sk.strip_prefix(Self::SNAP_PREFIX)?;
        let parts: Vec<&str> = rest.splitn(2, '#').collect();
        parts.first().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_pk_format() {
        assert_eq!(Keys::doc_pk("my-doc"), "DOC#my-doc");
    }

    #[test]
    fn operation_gsi1_pk_format() {
        let id = Uuid::nil();
        assert_eq!(
            Keys::operation_gsi1_pk(&id),
            "OP#00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn actor_gsi2_pk_format() {
        assert_eq!(
            Keys::actor_gsi2_pk("doc-1", "alice"),
            "ACTOR#doc-1#alice"
        );
    }

    #[test]
    fn parse_operation_sk() {
        let sk = "OP#2024-01-01T00:00:00.000Z/node1/1#550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(
            Keys::parse_operation_sk_hlc(sk),
            Some("2024-01-01T00:00:00.000Z/node1/1")
        );
        assert_eq!(
            Keys::parse_operation_sk_id(sk),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
    }
}
