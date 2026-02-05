//! Error types for conflux-core.

/// Top-level error type for conflux-core operations.
#[derive(Debug, thiserror::Error)]
pub enum ConfluxError {
    /// A merge operation failed.
    #[error(transparent)]
    Merge(#[from] MergeError),

    /// An operation could not be applied.
    #[error(transparent)]
    Operation(#[from] OperationError),

    /// A validation check failed.
    #[error(transparent)]
    Validation(#[from] ValidationError),

    /// A clock operation failed.
    #[error(transparent)]
    Clock(#[from] ClockError),
}

/// Errors during CRDT merge operations.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    /// Entity types do not match for merge.
    #[error("entity type mismatch: expected '{expected}', got '{actual}'")]
    EntityTypeMismatch { expected: String, actual: String },

    /// Merge strategies differ for the same field.
    #[error("strategy mismatch on field '{field}': {reason}")]
    StrategyMismatch { field: String, reason: String },

    /// Numeric comparison failed (e.g., comparing non-numeric FieldValues in Max/Min).
    #[error("numeric comparison failed on field '{field}': {reason}")]
    NumericComparisonFailed { field: String, reason: String },
}

/// Errors when applying operations to a document.
#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    /// The target entity was not found.
    #[error("entity not found: {0}")]
    EntityNotFound(String),

    /// Attempted to insert an entity that already exists.
    #[error("duplicate entity: {0}")]
    DuplicateEntity(String),

    /// The parent entity was not found.
    #[error("parent entity not found: {0}")]
    ParentNotFound(String),

    /// The field is not defined in the schema for this entity type.
    #[error("unknown field '{field}' on entity type '{entity_type}'")]
    UnknownField { entity_type: String, field: String },

    /// The entity type is not defined in the schema.
    #[error("unknown entity type: {0}")]
    UnknownEntityType(String),

    /// The environment is not defined in the schema.
    #[error("unknown environment: {0}")]
    UnknownEnvironment(String),

    /// The operation kind is not yet implemented.
    #[error("not implemented: {0}")]
    NotImplemented(String),

    /// Field value type does not match schema expectation.
    #[error("type mismatch on field '{field}': expected {expected}, got {actual}")]
    TypeMismatch {
        field: String,
        expected: String,
        actual: String,
    },

    /// Attempted to resolve a conflict on a field that has no conflict.
    #[error("no conflict on field '{field}' of entity '{entity_id}'")]
    NotAConflict { entity_id: String, field: String },
}

/// Errors during schema or document validation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// A required field is missing.
    #[error("missing required field '{field}' on entity '{entity_id}'")]
    MissingField { entity_id: String, field: String },

    /// A field value violates a constraint.
    #[error("constraint violation on '{field}': {reason}")]
    ConstraintViolation { field: String, reason: String },

    /// A reference points to a nonexistent entity.
    #[error("broken reference: field '{field}' references nonexistent entity '{target}'")]
    BrokenReference { field: String, target: String },
}

/// Errors from the HLC clock.
#[derive(Debug, thiserror::Error)]
pub enum ClockError {
    /// The incoming timestamp exceeds the maximum allowed delta.
    #[error("timestamp delta exceeded: {0}")]
    DeltaExceeded(String),
}
