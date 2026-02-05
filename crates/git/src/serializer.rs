//! Serializer plugin API for custom output formats.
//!
//! This module provides a trait-based plugin system for output format serializers,
//! along with a thread-safe registry for registering and retrieving them.
//!
//! # Built-in Serializers
//!
//! The following serializers are registered by default:
//! - YAML (`.yaml`, `.yml`)
//! - JSON (`.json`)
//! - TOML (`.toml`)
//! - KDL (`.kdl`)
//! - XML (`.xml`)
//! - HCL (`.tf`, `.hcl`)
//!
//! # Custom Serializers
//!
//! Implement the [`OutputSerializer`] trait and register with [`register_serializer`]:
//!
//! ```rust,no_run
//! use conflux_git::{OutputSerializer, register_serializer, GitError};
//! use serde::Serialize;
//! use std::sync::Arc;
//!
//! struct ProtobufSerializer;
//!
//! impl OutputSerializer for ProtobufSerializer {
//!     fn format_id(&self) -> &str { "protobuf" }
//!     fn extension(&self) -> &str { "pb" }
//!     fn display_name(&self) -> &str { "Protocol Buffers" }
//!
//!     fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
//!         // Custom protobuf serialization
//!         todo!()
//!     }
//! }
//!
//! register_serializer(Arc::new(ProtobufSerializer));
//! ```

use crate::error::GitError;
use crate::format::{json_to_hcl_impl, json_to_kdl_impl, json_to_xml_impl};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Trait for custom output format serializers.
///
/// Implement this trait to add support for new output formats. Each serializer
/// must be thread-safe (`Send + Sync`) to support concurrent access from multiple
/// threads.
pub trait OutputSerializer: Send + Sync {
    /// Returns the unique format identifier (e.g., "yaml", "protobuf").
    ///
    /// This is used as the key for registry lookup.
    fn format_id(&self) -> &str;

    /// Returns the canonical file extension for this format (e.g., "yaml", "pb").
    ///
    /// Do not include the leading dot.
    fn extension(&self) -> &str;

    /// Returns a human-readable name for this format (e.g., "YAML", "Protocol Buffers").
    fn display_name(&self) -> &str;

    /// Returns additional file extensions that map to this serializer.
    ///
    /// For example, YAML might return `["yml"]` in addition to its canonical "yaml" extension.
    fn alternate_extensions(&self) -> Vec<&str> {
        vec![]
    }

    /// Serializes a value to a string representation.
    ///
    /// The value is passed as an `erased_serde::Serialize` trait object to allow
    /// serializers to work with any serializable type.
    ///
    /// # Errors
    ///
    /// Returns `GitError::SerializationError` if serialization fails.
    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError>;
}

/// Extension trait for type-safe serialization.
pub trait OutputSerializerExt: OutputSerializer {
    /// Serializes a typed value to a string.
    fn serialize<T: Serialize>(&self, value: &T) -> Result<String, GitError> {
        self.serialize_value(value)
    }
}

impl<S: OutputSerializer + ?Sized> OutputSerializerExt for S {}

/// Thread-safe registry for output format serializers.
///
/// The registry maintains a mapping from format IDs to serializers, as well as
/// an index of file extensions for format detection.
pub struct SerializerRegistry {
    /// Map from format ID to serializer.
    serializers: RwLock<HashMap<String, Arc<dyn OutputSerializer>>>,
    /// Map from file extension to format ID.
    extension_index: RwLock<HashMap<String, String>>,
}

impl SerializerRegistry {
    /// Creates a new empty registry.
    pub fn new() -> Self {
        Self {
            serializers: RwLock::new(HashMap::new()),
            extension_index: RwLock::new(HashMap::new()),
        }
    }

    /// Creates a new registry with all built-in serializers pre-registered.
    pub fn with_builtins() -> Self {
        let registry = Self::new();

        registry.register(Arc::new(YamlSerializer));
        registry.register(Arc::new(JsonSerializer));
        registry.register(Arc::new(TomlSerializer));
        registry.register(Arc::new(KdlSerializer));
        registry.register(Arc::new(XmlSerializer));
        registry.register(Arc::new(HclSerializer));

        registry
    }

    /// Registers a serializer in the registry.
    ///
    /// The serializer's format ID becomes the primary lookup key. Its canonical
    /// extension and any alternate extensions are indexed for extension-based lookup.
    ///
    /// If a serializer with the same format ID already exists, it is replaced.
    pub fn register(&self, serializer: Arc<dyn OutputSerializer>) {
        let format_id = serializer.format_id().to_string();

        // Index extensions
        {
            let mut ext_idx = self.extension_index.write().unwrap();
            ext_idx.insert(serializer.extension().to_lowercase(), format_id.clone());
            for ext in serializer.alternate_extensions() {
                ext_idx.insert(ext.to_lowercase(), format_id.clone());
            }
        }

        // Store serializer
        {
            let mut serializers = self.serializers.write().unwrap();
            serializers.insert(format_id, serializer);
        }
    }

    /// Retrieves a serializer by its format ID.
    ///
    /// Returns `None` if no serializer is registered for the given format ID.
    pub fn get(&self, format_id: &str) -> Option<Arc<dyn OutputSerializer>> {
        let serializers = self.serializers.read().unwrap();
        serializers.get(format_id).cloned()
    }

    /// Retrieves a serializer by file extension.
    ///
    /// The extension should not include a leading dot. Lookup is case-insensitive.
    ///
    /// Returns `None` if no serializer is registered for the given extension.
    pub fn get_by_extension(&self, ext: &str) -> Option<Arc<dyn OutputSerializer>> {
        let ext_idx = self.extension_index.read().unwrap();
        let format_id = ext_idx.get(&ext.to_lowercase())?;

        let serializers = self.serializers.read().unwrap();
        serializers.get(format_id).cloned()
    }

    /// Returns the format ID associated with a file extension.
    ///
    /// Returns `None` if the extension is not recognized.
    pub fn format_id_for_extension(&self, ext: &str) -> Option<String> {
        let ext_idx = self.extension_index.read().unwrap();
        ext_idx.get(&ext.to_lowercase()).cloned()
    }

    /// Returns a list of all registered format IDs.
    pub fn format_ids(&self) -> Vec<String> {
        let serializers = self.serializers.read().unwrap();
        serializers.keys().cloned().collect()
    }

    /// Returns a list of all registered extensions.
    pub fn extensions(&self) -> Vec<String> {
        let ext_idx = self.extension_index.read().unwrap();
        ext_idx.keys().cloned().collect()
    }
}

impl Default for SerializerRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

lazy_static::lazy_static! {
    /// The global serializer registry.
    ///
    /// This registry is initialized with all built-in serializers and is used by
    /// default when no explicit registry is provided.
    static ref GLOBAL_REGISTRY: SerializerRegistry = SerializerRegistry::with_builtins();
}

/// Returns a reference to the global serializer registry.
///
/// The global registry is initialized with all built-in serializers.
pub fn global_registry() -> &'static SerializerRegistry {
    &GLOBAL_REGISTRY
}

/// Registers a serializer in the global registry.
///
/// This is a convenience function equivalent to `global_registry().register(serializer)`.
pub fn register_serializer(serializer: Arc<dyn OutputSerializer>) {
    GLOBAL_REGISTRY.register(serializer);
}

// ============================================================================
// Built-in Serializers
// ============================================================================

/// YAML format serializer.
pub struct YamlSerializer;

impl OutputSerializer for YamlSerializer {
    fn format_id(&self) -> &str {
        "yaml"
    }

    fn extension(&self) -> &str {
        "yaml"
    }

    fn display_name(&self) -> &str {
        "YAML"
    }

    fn alternate_extensions(&self) -> Vec<&str> {
        vec!["yml"]
    }

    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
        let mut buf = Vec::new();
        let mut serializer = serde_yaml::Serializer::new(&mut buf);
        value
            .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
            .map_err(|e| GitError::SerializationError(e.to_string()))?;
        String::from_utf8(buf).map_err(|e| GitError::SerializationError(e.to_string()))
    }
}

/// JSON format serializer.
pub struct JsonSerializer;

impl OutputSerializer for JsonSerializer {
    fn format_id(&self) -> &str {
        "json"
    }

    fn extension(&self) -> &str {
        "json"
    }

    fn display_name(&self) -> &str {
        "JSON"
    }

    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
        let mut buf = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"  ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
        value
            .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
            .map_err(|e| GitError::SerializationError(e.to_string()))?;
        String::from_utf8(buf).map_err(|e| GitError::SerializationError(e.to_string()))
    }
}

/// TOML format serializer.
pub struct TomlSerializer;

impl OutputSerializer for TomlSerializer {
    fn format_id(&self) -> &str {
        "toml"
    }

    fn extension(&self) -> &str {
        "toml"
    }

    fn display_name(&self) -> &str {
        "TOML"
    }

    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
        // TOML serialization requires going through serde_json::Value first
        // because toml crate doesn't support erased_serde directly
        let json_value = {
            let mut buf = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut buf);
            value
                .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
                .map_err(|e| GitError::SerializationError(e.to_string()))?;
            serde_json::from_slice::<serde_json::Value>(&buf)
                .map_err(|e| GitError::SerializationError(e.to_string()))?
        };

        // Convert JSON to TOML value
        let toml_value: toml::Value = serde_json::from_value(json_value)
            .map_err(|e| GitError::SerializationError(e.to_string()))?;

        toml::to_string_pretty(&toml_value).map_err(GitError::from)
    }
}

/// KDL format serializer.
pub struct KdlSerializer;

impl OutputSerializer for KdlSerializer {
    fn format_id(&self) -> &str {
        "kdl"
    }

    fn extension(&self) -> &str {
        "kdl"
    }

    fn display_name(&self) -> &str {
        "KDL"
    }

    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
        // Convert to JSON first, then to KDL representation
        let json_value = {
            let mut buf = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut buf);
            value
                .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
                .map_err(|e| GitError::SerializationError(e.to_string()))?;
            serde_json::from_slice::<serde_json::Value>(&buf)
                .map_err(|e| GitError::SerializationError(e.to_string()))?
        };

        let mut output = String::new();
        json_to_kdl_impl(&json_value, &mut output, 0, None);
        Ok(output)
    }
}

/// XML format serializer.
pub struct XmlSerializer;

impl OutputSerializer for XmlSerializer {
    fn format_id(&self) -> &str {
        "xml"
    }

    fn extension(&self) -> &str {
        "xml"
    }

    fn display_name(&self) -> &str {
        "XML"
    }

    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
        // Convert to JSON first, then to XML representation
        let json_value = {
            let mut buf = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut buf);
            value
                .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
                .map_err(|e| GitError::SerializationError(e.to_string()))?;
            serde_json::from_slice::<serde_json::Value>(&buf)
                .map_err(|e| GitError::SerializationError(e.to_string()))?
        };

        let mut output = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        json_to_xml_impl(&json_value, &mut output, "config", 0);
        Ok(output)
    }
}

/// HCL (Terraform) format serializer.
pub struct HclSerializer;

impl OutputSerializer for HclSerializer {
    fn format_id(&self) -> &str {
        "hcl"
    }

    fn extension(&self) -> &str {
        "tf"
    }

    fn display_name(&self) -> &str {
        "HCL"
    }

    fn alternate_extensions(&self) -> Vec<&str> {
        vec!["hcl"]
    }

    fn serialize_value(&self, value: &dyn erased_serde::Serialize) -> Result<String, GitError> {
        // Convert to JSON first, then to HCL representation
        let json_value = {
            let mut buf = Vec::new();
            let mut serializer = serde_json::Serializer::new(&mut buf);
            value
                .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
                .map_err(|e| GitError::SerializationError(e.to_string()))?;
            serde_json::from_slice::<serde_json::Value>(&buf)
                .map_err(|e| GitError::SerializationError(e.to_string()))?
        };

        let mut output = String::new();
        json_to_hcl_impl(&json_value, &mut output, 0, None);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_builtins() {
        let r = SerializerRegistry::with_builtins();
        assert!(r.get("yaml").is_some());
        assert!(r.get("json").is_some());
        assert!(r.get("toml").is_some());
        assert!(r.get("kdl").is_some());
        assert!(r.get("xml").is_some());
        assert!(r.get("hcl").is_some());
    }

    #[test]
    fn registry_extension_lookup() {
        let r = SerializerRegistry::with_builtins();

        // Primary extensions
        assert!(r.get_by_extension("yaml").is_some());
        assert!(r.get_by_extension("json").is_some());
        assert!(r.get_by_extension("tf").is_some());

        // Alternate extensions
        assert!(r.get_by_extension("yml").is_some());
        assert!(r.get_by_extension("hcl").is_some());

        // Case insensitive
        assert!(r.get_by_extension("YAML").is_some());
        assert!(r.get_by_extension("JSON").is_some());
    }

    #[test]
    fn registry_custom_serializer() {
        struct CustomSerializer;
        impl OutputSerializer for CustomSerializer {
            fn format_id(&self) -> &str {
                "custom"
            }
            fn extension(&self) -> &str {
                "cust"
            }
            fn display_name(&self) -> &str {
                "Custom Format"
            }
            fn serialize_value(
                &self,
                value: &dyn erased_serde::Serialize,
            ) -> Result<String, GitError> {
                let mut buf = Vec::new();
                let mut serializer = serde_json::Serializer::new(&mut buf);
                value
                    .erased_serialize(&mut <dyn erased_serde::Serializer>::erase(&mut serializer))
                    .map_err(|e| GitError::SerializationError(e.to_string()))?;
                let json = String::from_utf8(buf).unwrap();
                Ok(format!("CUSTOM:{json}"))
            }
        }

        let r = SerializerRegistry::new();
        r.register(Arc::new(CustomSerializer));

        let serializer = r.get("custom").unwrap();
        assert_eq!(serializer.format_id(), "custom");
        assert_eq!(serializer.extension(), "cust");
        assert_eq!(serializer.display_name(), "Custom Format");

        let result = serializer.serialize(&serde_json::json!({"test": 42})).unwrap();
        assert!(result.starts_with("CUSTOM:"));
        assert!(result.contains("test"));
    }

    #[test]
    fn yaml_serializer_output() {
        let s = YamlSerializer;
        let data = serde_json::json!({"name": "test", "value": 42});
        let result = s.serialize(&data).unwrap();
        assert!(result.contains("name: test"));
        assert!(result.contains("value: 42"));
    }

    #[test]
    fn json_serializer_output() {
        let s = JsonSerializer;
        let data = serde_json::json!({"name": "test", "value": 42});
        let result = s.serialize(&data).unwrap();
        assert!(result.contains("\"name\": \"test\""));
        assert!(result.contains("\"value\": 42"));
    }

    #[test]
    fn toml_serializer_output() {
        let s = TomlSerializer;
        let data = serde_json::json!({"name": "test", "value": 42});
        let result = s.serialize(&data).unwrap();
        assert!(result.contains("name = \"test\""));
        assert!(result.contains("value = 42"));
    }

    #[test]
    fn kdl_serializer_output() {
        let s = KdlSerializer;
        let data = serde_json::json!({"name": "test", "value": 42});
        let result = s.serialize(&data).unwrap();
        assert!(result.contains("name \"test\""));
        assert!(result.contains("value 42"));
    }

    #[test]
    fn xml_serializer_output() {
        let s = XmlSerializer;
        let data = serde_json::json!({"name": "test", "value": 42});
        let result = s.serialize(&data).unwrap();
        assert!(result.contains("<?xml version=\"1.0\""));
        assert!(result.contains("<name>test</name>"));
        assert!(result.contains("<value>42</value>"));
    }

    #[test]
    fn hcl_serializer_output() {
        let s = HclSerializer;
        let data = serde_json::json!({"name": "test", "value": 42});
        let result = s.serialize(&data).unwrap();
        assert!(result.contains("name = \"test\""));
        assert!(result.contains("value = 42"));
    }

    #[test]
    fn global_registry_works() {
        let yaml = global_registry().get("yaml");
        assert!(yaml.is_some());
    }

    #[test]
    fn format_ids_list() {
        let r = SerializerRegistry::with_builtins();
        let ids = r.format_ids();
        assert!(ids.contains(&"yaml".to_string()));
        assert!(ids.contains(&"json".to_string()));
        assert!(ids.contains(&"toml".to_string()));
    }
}
