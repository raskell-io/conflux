//! Error types for conflux-schema.

/// Errors that can occur when loading or validating a schema.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// Failed to read the schema file.
    #[error("failed to read schema file: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to parse TOML.
    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    /// The schema is missing a required section or field.
    #[error("missing required schema field: {0}")]
    MissingField(String),

    /// An invalid value was encountered during parsing.
    #[error("invalid value in schema: {0}")]
    InvalidValue(String),

    /// An unknown field type was specified.
    #[error("unknown field type '{field_type}' on entity '{entity_type}', field '{field}'")]
    UnknownFieldType {
        entity_type: String,
        field: String,
        field_type: String,
    },

    /// An unknown merge strategy was specified.
    #[error("unknown merge strategy '{strategy}' on entity '{entity_type}', field '{field}'")]
    UnknownMergeStrategy {
        entity_type: String,
        field: String,
        strategy: String,
    },

    /// An unknown conflict mode was specified.
    #[error("unknown conflict mode '{mode}' on entity '{entity_type}', field '{field}'")]
    UnknownConflictMode {
        entity_type: String,
        field: String,
        mode: String,
    },

    /// A schema consistency check failed.
    #[error("schema validation error: {0}")]
    Validation(String),
}
