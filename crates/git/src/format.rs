//! Output format serialization for config files.
//!
//! This module provides the original enum-based serialization API for backward
//! compatibility. For the extensible plugin-based API, see the [`serializer`](crate::serializer)
//! module.

use crate::error::GitError;
use crate::serializer::{global_registry, OutputSerializerExt, SerializerRegistry};
use serde::Serialize;
use std::collections::BTreeMap;

/// Supported output file formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormat {
    Yaml,
    Json,
    Toml,
    Kdl,
    Xml,
    Hcl,
}

impl OutputFormat {
    /// Parses an output format from a file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "yaml" | "yml" => Some(OutputFormat::Yaml),
            "json" => Some(OutputFormat::Json),
            "toml" => Some(OutputFormat::Toml),
            "kdl" => Some(OutputFormat::Kdl),
            "xml" => Some(OutputFormat::Xml),
            "tf" | "hcl" => Some(OutputFormat::Hcl),
            _ => None,
        }
    }

    /// Returns the canonical file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            OutputFormat::Yaml => "yaml",
            OutputFormat::Json => "json",
            OutputFormat::Toml => "toml",
            OutputFormat::Kdl => "kdl",
            OutputFormat::Xml => "xml",
            OutputFormat::Hcl => "tf",
        }
    }

    /// Returns the format name.
    pub fn name(&self) -> &'static str {
        match self {
            OutputFormat::Yaml => "YAML",
            OutputFormat::Json => "JSON",
            OutputFormat::Toml => "TOML",
            OutputFormat::Kdl => "KDL",
            OutputFormat::Xml => "XML",
            OutputFormat::Hcl => "HCL",
        }
    }
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl OutputFormat {
    /// Returns the format ID string used by the serializer registry.
    pub fn as_str(&self) -> &'static str {
        match self {
            OutputFormat::Yaml => "yaml",
            OutputFormat::Json => "json",
            OutputFormat::Toml => "toml",
            OutputFormat::Kdl => "kdl",
            OutputFormat::Xml => "xml",
            OutputFormat::Hcl => "hcl",
        }
    }
}

/// Serializes a value to the specified output format.
///
/// # Errors
///
/// Returns `GitError` if serialization fails for the given format.
pub fn serialize<T: Serialize>(value: &T, format: OutputFormat) -> Result<String, GitError> {
    match format {
        OutputFormat::Yaml => {
            let s = serde_yaml::to_string(value)?;
            Ok(s)
        }
        OutputFormat::Json => {
            let s = serde_json::to_string_pretty(value)?;
            Ok(s)
        }
        OutputFormat::Toml => {
            let s = toml::to_string_pretty(value)?;
            Ok(s)
        }
        OutputFormat::Kdl => {
            // KDL doesn't have direct serde support for arbitrary types.
            // We convert through JSON then to a KDL-like representation.
            let json = serde_json::to_value(value)?;
            let kdl = json_to_kdl(&json);
            Ok(kdl)
        }
        OutputFormat::Xml => {
            // quick-xml's serialize works best with named root elements.
            // For now, we wrap in a generic <config> element.
            let json = serde_json::to_value(value)?;
            let xml = json_to_xml(&json, "config");
            Ok(xml)
        }
        OutputFormat::Hcl => {
            // hcl-rs serialization. We convert through JSON.
            let json = serde_json::to_value(value)?;
            let hcl = json_to_hcl(&json);
            Ok(hcl)
        }
    }
}

/// Serializes a value using the global serializer registry.
///
/// This function looks up the serializer by format ID and delegates to it.
/// Use this for extensible format support.
///
/// # Errors
///
/// Returns `GitError::UnknownFormat` if no serializer is registered for the format ID.
/// Returns `GitError::SerializationError` if serialization fails.
pub fn serialize_with_registry<T: Serialize>(
    value: &T,
    format_id: &str,
    registry: &SerializerRegistry,
) -> Result<String, GitError> {
    let serializer = registry
        .get(format_id)
        .ok_or_else(|| GitError::UnknownFormat(format_id.to_string()))?;
    serializer.serialize(value)
}

/// Serializes a value using the global registry by format ID.
///
/// Convenience wrapper around [`serialize_with_registry`] using the global registry.
pub fn serialize_by_format_id<T: Serialize>(value: &T, format_id: &str) -> Result<String, GitError> {
    serialize_with_registry(value, format_id, global_registry())
}

/// Converts a JSON value to a KDL-like string representation.
fn json_to_kdl(value: &serde_json::Value) -> String {
    let mut output = String::new();
    json_to_kdl_impl(value, &mut output, 0, None);
    output
}

pub(crate) fn json_to_kdl_impl(
    value: &serde_json::Value,
    output: &mut String,
    indent: usize,
    name: Option<&str>,
) {
    let prefix = "  ".repeat(indent);
    match value {
        serde_json::Value::Object(map) => {
            if let Some(n) = name {
                output.push_str(&format!("{prefix}{n} {{\n"));
                for (k, v) in map {
                    json_to_kdl_impl(v, output, indent + 1, Some(k));
                }
                output.push_str(&format!("{prefix}}}\n"));
            } else {
                for (k, v) in map {
                    json_to_kdl_impl(v, output, indent, Some(k));
                }
            }
        }
        serde_json::Value::Array(arr) => {
            if let Some(n) = name {
                for item in arr {
                    json_to_kdl_impl(item, output, indent, Some(n));
                }
            } else {
                for item in arr {
                    json_to_kdl_impl(item, output, indent, None);
                }
            }
        }
        serde_json::Value::String(s) => {
            if let Some(n) = name {
                output.push_str(&format!("{prefix}{n} \"{}\"\n", escape_kdl_string(s)));
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(name) = name {
                output.push_str(&format!("{prefix}{name} {n}\n"));
            }
        }
        serde_json::Value::Bool(b) => {
            if let Some(name) = name {
                output.push_str(&format!("{prefix}{name} {b}\n"));
            }
        }
        serde_json::Value::Null => {
            if let Some(name) = name {
                output.push_str(&format!("{prefix}{name} null\n"));
            }
        }
    }
}

pub(crate) fn escape_kdl_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Converts a JSON value to XML.
fn json_to_xml(value: &serde_json::Value, root_name: &str) -> String {
    let mut output = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    json_to_xml_impl(value, &mut output, root_name, 0);
    output
}

pub(crate) fn json_to_xml_impl(value: &serde_json::Value, output: &mut String, name: &str, indent: usize) {
    let prefix = "  ".repeat(indent);
    match value {
        serde_json::Value::Object(map) => {
            output.push_str(&format!("{prefix}<{name}>\n"));
            for (k, v) in map {
                json_to_xml_impl(v, output, k, indent + 1);
            }
            output.push_str(&format!("{prefix}</{name}>\n"));
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                json_to_xml_impl(item, output, name, indent);
            }
        }
        serde_json::Value::String(s) => {
            output.push_str(&format!("{prefix}<{name}>{}</{name}>\n", escape_xml(s)));
        }
        serde_json::Value::Number(n) => {
            output.push_str(&format!("{prefix}<{name}>{n}</{name}>\n"));
        }
        serde_json::Value::Bool(b) => {
            output.push_str(&format!("{prefix}<{name}>{b}</{name}>\n"));
        }
        serde_json::Value::Null => {
            output.push_str(&format!("{prefix}<{name}/>\n"));
        }
    }
}

pub(crate) fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Converts a JSON value to HCL (Terraform) format.
fn json_to_hcl(value: &serde_json::Value) -> String {
    let mut output = String::new();
    json_to_hcl_impl(value, &mut output, 0, None);
    output
}

pub(crate) fn json_to_hcl_impl(
    value: &serde_json::Value,
    output: &mut String,
    indent: usize,
    name: Option<&str>,
) {
    let prefix = "  ".repeat(indent);
    match value {
        serde_json::Value::Object(map) => {
            if let Some(n) = name {
                output.push_str(&format!("{prefix}{n} = {{\n"));
                for (k, v) in map {
                    json_to_hcl_impl(v, output, indent + 1, Some(k));
                }
                output.push_str(&format!("{prefix}}}\n"));
            } else {
                for (k, v) in map {
                    json_to_hcl_impl(v, output, indent, Some(k));
                }
            }
        }
        serde_json::Value::Array(arr) => {
            if let Some(n) = name {
                output.push_str(&format!("{prefix}{n} = [\n"));
                for item in arr {
                    json_to_hcl_impl(item, output, indent + 1, None);
                    output.push_str(",\n");
                }
                output.push_str(&format!("{prefix}]\n"));
            }
        }
        serde_json::Value::String(s) => {
            if let Some(n) = name {
                output.push_str(&format!("{prefix}{n} = \"{}\"\n", escape_hcl_string(s)));
            } else {
                output.push_str(&format!("{prefix}\"{}\"\n", escape_hcl_string(s)));
            }
        }
        serde_json::Value::Number(n) => {
            if let Some(name) = name {
                output.push_str(&format!("{prefix}{name} = {n}\n"));
            } else {
                output.push_str(&format!("{prefix}{n}"));
            }
        }
        serde_json::Value::Bool(b) => {
            if let Some(name) = name {
                output.push_str(&format!("{prefix}{name} = {b}\n"));
            } else {
                output.push_str(&format!("{prefix}{b}"));
            }
        }
        serde_json::Value::Null => {
            if let Some(name) = name {
                output.push_str(&format!("{prefix}{name} = null\n"));
            } else {
                output.push_str(&format!("{prefix}null"));
            }
        }
    }
}

pub(crate) fn escape_hcl_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Represents a file to be written to the git repository.
#[derive(Debug, Clone)]
pub struct OutputFile {
    /// Relative path within the repository.
    pub path: String,
    /// File content.
    pub content: String,
    /// The format used.
    pub format: OutputFormat,
}

/// Serializes a document's entities to output files for a given environment.
///
/// This groups entities by type and serializes each group to a separate file.
pub fn serialize_document(
    entities: &BTreeMap<String, serde_json::Value>,
    environment: &str,
    format: OutputFormat,
) -> Result<Vec<OutputFile>, GitError> {
    // Group entities by type (first segment of entity_id before '.')
    let mut by_type: BTreeMap<String, BTreeMap<String, serde_json::Value>> = BTreeMap::new();

    for (entity_id, entity) in entities {
        let type_name = entity_id.split('.').next().unwrap_or(entity_id).to_string();
        by_type
            .entry(type_name)
            .or_default()
            .insert(entity_id.clone(), entity.clone());
    }

    let mut files = Vec::new();
    for (type_name, entities_of_type) in by_type {
        let content = serialize(&entities_of_type, format)?;
        let path = format!("{}/{}.{}", environment, type_name, format.extension());
        files.push(OutputFile {
            path,
            content,
            format,
        });
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_from_extension() {
        assert_eq!(
            OutputFormat::from_extension("yaml"),
            Some(OutputFormat::Yaml)
        );
        assert_eq!(
            OutputFormat::from_extension("yml"),
            Some(OutputFormat::Yaml)
        );
        assert_eq!(
            OutputFormat::from_extension("json"),
            Some(OutputFormat::Json)
        );
        assert_eq!(
            OutputFormat::from_extension("toml"),
            Some(OutputFormat::Toml)
        );
        assert_eq!(OutputFormat::from_extension("kdl"), Some(OutputFormat::Kdl));
        assert_eq!(OutputFormat::from_extension("xml"), Some(OutputFormat::Xml));
        assert_eq!(OutputFormat::from_extension("tf"), Some(OutputFormat::Hcl));
        assert_eq!(OutputFormat::from_extension("hcl"), Some(OutputFormat::Hcl));
        assert_eq!(OutputFormat::from_extension("unknown"), None);
    }

    #[test]
    fn serialize_yaml() {
        let data = serde_json::json!({
            "name": "test",
            "value": 42
        });
        let result = serialize(&data, OutputFormat::Yaml).unwrap();
        assert!(result.contains("name: test"));
        assert!(result.contains("value: 42"));
    }

    #[test]
    fn serialize_json() {
        let data = serde_json::json!({
            "name": "test",
            "value": 42
        });
        let result = serialize(&data, OutputFormat::Json).unwrap();
        assert!(result.contains("\"name\": \"test\""));
        assert!(result.contains("\"value\": 42"));
    }

    #[test]
    fn serialize_toml() {
        let data = serde_json::json!({
            "name": "test",
            "value": 42
        });
        let result = serialize(&data, OutputFormat::Toml).unwrap();
        assert!(result.contains("name = \"test\""));
        assert!(result.contains("value = 42"));
    }

    #[test]
    fn serialize_kdl() {
        let data = serde_json::json!({
            "name": "test",
            "value": 42
        });
        let result = serialize(&data, OutputFormat::Kdl).unwrap();
        assert!(result.contains("name \"test\""));
        assert!(result.contains("value 42"));
    }

    #[test]
    fn serialize_xml() {
        let data = serde_json::json!({
            "name": "test",
            "value": 42
        });
        let result = serialize(&data, OutputFormat::Xml).unwrap();
        assert!(result.contains("<name>test</name>"));
        assert!(result.contains("<value>42</value>"));
    }

    #[test]
    fn serialize_hcl() {
        let data = serde_json::json!({
            "name": "test",
            "value": 42
        });
        let result = serialize(&data, OutputFormat::Hcl).unwrap();
        assert!(result.contains("name = \"test\""));
        assert!(result.contains("value = 42"));
    }
}
