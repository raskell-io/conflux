//! Milestone projector — orchestrates serialization and git commits.

use crate::commit::CommitBuilder;
use crate::error::GitError;
use crate::format::{serialize_document, OutputFile, OutputFormat};
use conflux_core::{Document, FieldValue, HlcTimestamp, Operation};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Configuration for the milestone projector.
#[derive(Debug, Clone)]
pub struct ProjectorConfig {
    /// Path to the git repository.
    pub repo_path: PathBuf,
    /// Default output format for files.
    pub default_format: OutputFormat,
    /// Author name for commits.
    pub author_name: String,
    /// Author email for commits.
    pub author_email: String,
}

impl ProjectorConfig {
    /// Creates a new projector config with defaults.
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
            default_format: OutputFormat::Yaml,
            author_name: "Conflux".to_string(),
            author_email: "conflux@localhost".to_string(),
        }
    }

    /// Sets the default output format.
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.default_format = format;
        self
    }

    /// Sets the commit author.
    pub fn with_author(mut self, name: impl Into<String>, email: impl Into<String>) -> Self {
        self.author_name = name.into();
        self.author_email = email.into();
        self
    }
}

/// Result of a milestone projection.
#[derive(Debug, Clone)]
pub struct MilestoneResult {
    /// Git commit SHA (hex string).
    pub commit_sha: String,
    /// Files that were written.
    pub files_written: Vec<String>,
    /// Number of operations included.
    pub operation_count: usize,
    /// The causal range start.
    pub hlc_start: Option<HlcTimestamp>,
    /// The causal range end.
    pub hlc_end: Option<HlcTimestamp>,
}

/// Orchestrates milestone projection to a git repository.
pub struct MilestoneProjector {
    config: ProjectorConfig,
}

impl MilestoneProjector {
    /// Creates a new milestone projector.
    pub fn new(config: ProjectorConfig) -> Self {
        Self { config }
    }

    /// Projects the current document state to the git repository.
    ///
    /// # Arguments
    ///
    /// * `document` - The resolved document state
    /// * `operations` - Operations since the last milestone
    /// * `environments` - List of environments to project
    /// * `message` - Human-readable milestone message
    ///
    /// # Errors
    ///
    /// Returns `GitError` if serialization or git operations fail.
    pub fn project(
        &self,
        document: &Document,
        operations: Vec<Operation>,
        environments: &[String],
        message: &str,
    ) -> Result<MilestoneResult, GitError> {
        // Convert document to serializable format
        let entities = document_to_serializable(document);

        // Serialize to files for each environment
        let mut all_files = Vec::new();
        for env in environments {
            let files = serialize_document(&entities, env, self.config.default_format)?;
            all_files.extend(files);
        }

        // Write milestone metadata
        let metadata = self.create_milestone_metadata(&operations, environments);
        all_files.push(OutputFile {
            path: ".conflux/milestone.json".to_string(),
            content: serde_json::to_string_pretty(&metadata)?,
            format: OutputFormat::Json,
        });

        // Write files to disk
        let files_written = self.write_files(&all_files)?;

        // Build commit message
        let commit_msg = CommitBuilder::new(message)
            .with_operations(operations.clone())
            .with_environments(environments.to_vec())
            .build();

        // Commit to git
        let commit_sha = self.git_commit(&files_written, &commit_msg)?;

        // Determine causal range
        let hlc_start = operations.first().map(|op| op.timestamp);
        let hlc_end = operations.last().map(|op| op.timestamp);

        Ok(MilestoneResult {
            commit_sha,
            files_written,
            operation_count: operations.len(),
            hlc_start,
            hlc_end,
        })
    }

    /// Writes output files to the repository working directory.
    fn write_files(&self, files: &[OutputFile]) -> Result<Vec<String>, GitError> {
        let mut written = Vec::new();

        for file in files {
            let full_path = self.config.repo_path.join(&file.path);

            // Ensure parent directory exists
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&full_path, &file.content)?;
            written.push(file.path.clone());
        }

        Ok(written)
    }

    /// Creates milestone metadata JSON.
    fn create_milestone_metadata(
        &self,
        operations: &[Operation],
        environments: &[String],
    ) -> serde_json::Value {
        let hlc_start = operations.first().map(|op| op.timestamp.to_string());
        let hlc_end = operations.last().map(|op| op.timestamp.to_string());

        json!({
            "format_version": 1,
            "operation_count": operations.len(),
            "environments": environments,
            "hlc_range": {
                "start": hlc_start,
                "end": hlc_end
            },
            "created_at": chrono::Utc::now().to_rfc3339()
        })
    }

    /// Commits the specified files using git CLI.
    fn git_commit(&self, files: &[String], message: &str) -> Result<String, GitError> {
        // Check if this is a git repository
        let git_dir = self.config.repo_path.join(".git");
        if !git_dir.exists() {
            return Err(GitError::RepoNotFound(
                self.config.repo_path.display().to_string(),
            ));
        }

        // Stage files
        for file in files {
            let output = Command::new("git")
                .current_dir(&self.config.repo_path)
                .args(["add", file])
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(GitError::Git(format!("git add failed: {stderr}")));
            }
        }

        // Check if there are staged changes
        let status_output = Command::new("git")
            .current_dir(&self.config.repo_path)
            .args(["diff", "--cached", "--quiet"])
            .status()?;

        if status_output.success() {
            // No changes staged
            return Err(GitError::NothingToCommit);
        }

        // Commit with author info
        let author = format!("{} <{}>", self.config.author_name, self.config.author_email);
        let output = Command::new("git")
            .current_dir(&self.config.repo_path)
            .args(["commit", "--author", &author, "-m", message])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::Git(format!("git commit failed: {stderr}")));
        }

        // Get the commit SHA
        let sha_output = Command::new("git")
            .current_dir(&self.config.repo_path)
            .args(["rev-parse", "HEAD"])
            .output()?;

        if !sha_output.status.success() {
            let stderr = String::from_utf8_lossy(&sha_output.stderr);
            return Err(GitError::Git(format!("git rev-parse failed: {stderr}")));
        }

        let sha = String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string();
        Ok(sha)
    }

    /// Initializes a new git repository at the configured path.
    ///
    /// Useful for testing or initial setup.
    ///
    /// # Errors
    ///
    /// Returns `GitError` if git init fails.
    pub fn init_repo(&self) -> Result<(), GitError> {
        fs::create_dir_all(&self.config.repo_path)?;

        let output = Command::new("git")
            .current_dir(&self.config.repo_path)
            .args(["init"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError::Git(format!("git init failed: {stderr}")));
        }

        // Configure user for this repo (needed for commits)
        Command::new("git")
            .current_dir(&self.config.repo_path)
            .args(["config", "user.name", &self.config.author_name])
            .output()?;

        Command::new("git")
            .current_dir(&self.config.repo_path)
            .args(["config", "user.email", &self.config.author_email])
            .output()?;

        Ok(())
    }
}

/// Converts a Document to a serializable BTreeMap.
fn document_to_serializable(document: &Document) -> BTreeMap<String, serde_json::Value> {
    let mut result = BTreeMap::new();

    for (entity_id, entity) in &document.entities {
        // Skip tombstoned entities
        if entity.tombstone.value {
            continue;
        }

        let mut fields = serde_json::Map::new();
        fields.insert("_type".to_string(), json!(entity.entity_type));

        if let Some(parent) = &entity.parent_id {
            fields.insert("_parent".to_string(), json!(parent.to_string()));
        }

        for (field_name, field_state) in &entity.fields {
            let value = field_value_to_json(&field_state.value());
            fields.insert(field_name.clone(), value);
        }

        result.insert(entity_id.to_string(), serde_json::Value::Object(fields));
    }

    result
}

/// Converts a FieldValue to a JSON value.
fn field_value_to_json(value: &FieldValue) -> serde_json::Value {
    match value {
        FieldValue::String(s) => json!(s),
        FieldValue::Int(i) => json!(i),
        FieldValue::Float(f) => json!(f),
        FieldValue::Bool(b) => json!(b),
        FieldValue::Duration(d) => json!(d),
        FieldValue::Ref(r) => json!(r.to_string()),
        FieldValue::List(items) => {
            let arr: Vec<_> = items.iter().map(field_value_to_json).collect();
            json!(arr)
        }
        FieldValue::Map(map) => {
            let obj: serde_json::Map<_, _> = map
                .iter()
                .map(|(k, v)| (k.clone(), field_value_to_json(v)))
                .collect();
            json!(obj)
        }
        FieldValue::Null => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conflux_core::{ActorClass, ActorId, Clock, Entity, EntityId};

    fn test_actor() -> ActorId {
        ActorId::new("test", ActorClass::System)
    }

    #[test]
    fn document_to_serializable_empty() {
        let doc = Document::new();
        let result = document_to_serializable(&doc);
        assert!(result.is_empty());
    }

    #[test]
    fn document_to_serializable_with_entity() {
        let clock = Clock::new();
        let actor = test_actor();
        let mut doc = Document::new();

        let entity = Entity::new(
            EntityId::new("route.api"),
            "route".to_string(),
            clock.new_timestamp(),
            actor.clone(),
        );
        doc.entities.insert(EntityId::new("route.api"), entity);

        let result = document_to_serializable(&doc);
        assert_eq!(result.len(), 1);
        assert!(result.contains_key("route.api"));

        let entity_json = &result["route.api"];
        assert_eq!(entity_json["_type"], "route");
    }

    #[test]
    fn projector_config_builder() {
        let config = ProjectorConfig::new("/tmp/repo")
            .with_format(OutputFormat::Json)
            .with_author("Test User", "test@example.com");

        assert_eq!(config.default_format, OutputFormat::Json);
        assert_eq!(config.author_name, "Test User");
        assert_eq!(config.author_email, "test@example.com");
    }

    #[test]
    fn projector_with_real_git() {
        use std::env;

        // Create a temp directory for the test repo
        let temp_dir = env::temp_dir().join(format!("conflux-git-test-{}", uuid::Uuid::new_v4()));

        let config = ProjectorConfig::new(&temp_dir)
            .with_format(OutputFormat::Yaml)
            .with_author("Test", "test@test.com");

        let projector = MilestoneProjector::new(config);

        // Initialize repo
        projector.init_repo().unwrap();

        // Create a document with an entity
        let clock = Clock::new();
        let actor = test_actor();
        let mut doc = Document::new();

        let entity = Entity::new(
            EntityId::new("route.api"),
            "route".to_string(),
            clock.new_timestamp(),
            actor.clone(),
        );
        doc.entities.insert(EntityId::new("route.api"), entity);

        // Project
        let result = projector
            .project(&doc, vec![], &["production".to_string()], "initial commit")
            .unwrap();

        assert!(!result.commit_sha.is_empty());
        assert!(!result.files_written.is_empty());

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
