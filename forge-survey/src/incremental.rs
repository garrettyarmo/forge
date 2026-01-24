//! Incremental survey support for Forge.
//!
//! This module provides functionality for efficient re-surveys by tracking
//! file changes between survey runs. It uses git commit SHAs to detect
//! repository-level changes and git diff to identify specific file changes.
//!
//! # Design
//!
//! The incremental survey process works as follows:
//!
//! 1. **First Survey**: Performs a full survey and stores state (commit SHAs, file hashes)
//! 2. **Subsequent Surveys**: Compares current state with stored state to detect changes
//! 3. **Selective Parsing**: Only re-parses files that have been added or modified
//! 4. **Graph Merging**: Merges new discoveries with the existing knowledge graph
//!
//! # Benefits
//!
//! - **Speed**: >10x faster for unchanged repositories
//! - **Efficiency**: Only processes changed files
//! - **Preservation**: Business context annotations survive re-surveys

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::process::Command;

/// Persistent state for incremental surveys.
///
/// This struct is serialized to `.forge/survey-state.json` and tracks
/// the state of each repository at the time of the last survey.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurveyState {
    /// Version of the state format (for future compatibility)
    pub version: u32,

    /// When the last full survey was run
    pub last_full_survey: DateTime<Utc>,

    /// When the state was last updated
    pub last_updated: DateTime<Utc>,

    /// State for each repository (keyed by full_name e.g., "owner/repo")
    pub repos: HashMap<String, RepoState>,
}

/// State for a single repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoState {
    /// Git commit SHA at last survey
    pub commit_sha: String,

    /// When this repo was last surveyed
    pub last_surveyed: DateTime<Utc>,

    /// Number of discoveries found from this repo
    pub discovery_count: usize,

    /// Languages detected in this repo
    pub detected_languages: Vec<String>,

    /// Whether this repo was successfully surveyed
    pub survey_successful: bool,
}

impl SurveyState {
    /// Create a new empty survey state.
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            version: 1,
            last_full_survey: now,
            last_updated: now,
            repos: HashMap::new(),
        }
    }

    /// Load state from a JSON file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the state file
    ///
    /// # Returns
    ///
    /// The loaded state, or an error if the file doesn't exist or is invalid.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        let content = std::fs::read_to_string(path)?;
        let state: Self = serde_json::from_str(&content)?;
        Ok(state)
    }

    /// Save state to a JSON file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to save the state file
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the state for a specific repository.
    pub fn get_repo(&self, repo_name: &str) -> Option<&RepoState> {
        self.repos.get(repo_name)
    }

    /// Update the state for a repository after surveying.
    ///
    /// # Arguments
    ///
    /// * `repo_name` - The full repository name (e.g., "owner/repo")
    /// * `commit_sha` - The current commit SHA
    /// * `discovery_count` - Number of discoveries found
    /// * `detected_languages` - Languages detected in the repo
    /// * `successful` - Whether the survey completed successfully
    pub fn mark_surveyed(
        &mut self,
        repo_name: &str,
        commit_sha: &str,
        discovery_count: usize,
        detected_languages: Vec<String>,
        successful: bool,
    ) {
        self.repos.insert(
            repo_name.to_string(),
            RepoState {
                commit_sha: commit_sha.to_string(),
                last_surveyed: Utc::now(),
                discovery_count,
                detected_languages,
                survey_successful: successful,
            },
        );
        self.last_updated = Utc::now();
    }

    /// Check if a repository needs to be re-surveyed based on commit SHA.
    ///
    /// Returns `true` if:
    /// - The repo has never been surveyed
    /// - The repo's commit SHA has changed
    /// - The previous survey failed
    pub fn needs_survey(&self, repo_name: &str, current_sha: &str) -> bool {
        match self.repos.get(repo_name) {
            None => true,
            Some(state) => !state.survey_successful || state.commit_sha != current_sha,
        }
    }

    /// Get the number of repositories that have been surveyed.
    pub fn repo_count(&self) -> usize {
        self.repos.len()
    }

    /// Get the total number of discoveries across all repos.
    pub fn total_discoveries(&self) -> usize {
        self.repos.values().map(|r| r.discovery_count).sum()
    }

    /// Mark the start of a full survey.
    pub fn mark_full_survey_start(&mut self) {
        self.last_full_survey = Utc::now();
    }
}

impl Default for SurveyState {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of change detection for a repository.
#[derive(Debug, Clone)]
pub struct ChangeResult {
    /// Files that were added since last survey
    pub added: Vec<PathBuf>,

    /// Files that were modified since last survey
    pub modified: Vec<PathBuf>,

    /// Files that were deleted since last survey
    pub deleted: Vec<PathBuf>,

    /// Current commit SHA
    pub current_sha: String,

    /// Previous commit SHA (if known)
    pub previous_sha: Option<String>,

    /// Whether a full re-survey is needed (e.g., first survey, force push)
    pub needs_full_survey: bool,

    /// Reason for needing full survey (if applicable)
    pub full_survey_reason: Option<String>,
}

impl ChangeResult {
    /// Check if there are any changes that require parsing.
    pub fn has_changes(&self) -> bool {
        !self.added.is_empty() || !self.modified.is_empty() || !self.deleted.is_empty()
    }

    /// Get total number of changed files.
    pub fn change_count(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }

    /// Get all files that need to be parsed (added + modified).
    pub fn files_to_parse(&self) -> Vec<&PathBuf> {
        self.added.iter().chain(self.modified.iter()).collect()
    }
}

/// Detects changes in repositories between survey runs.
pub struct ChangeDetector {
    state: SurveyState,
}

impl ChangeDetector {
    /// Create a new change detector with the given survey state.
    pub fn new(state: SurveyState) -> Self {
        Self { state }
    }

    /// Get the underlying state (for saving after detection).
    pub fn state(&self) -> &SurveyState {
        &self.state
    }

    /// Detect changes in a repository since the last survey.
    ///
    /// # Arguments
    ///
    /// * `repo_name` - The full repository name (e.g., "owner/repo")
    /// * `repo_path` - Local path to the repository
    ///
    /// # Returns
    ///
    /// A `ChangeResult` describing what has changed.
    pub async fn detect_changes(
        &self,
        repo_name: &str,
        repo_path: &Path,
    ) -> Result<ChangeResult, ChangeError> {
        // Get current commit SHA
        let current_sha = get_current_commit(repo_path).await?;

        // Check if we have previous state for this repo
        let repo_state = match self.state.get_repo(repo_name) {
            Some(s) => s,
            None => {
                // No previous state - needs full survey
                return Ok(ChangeResult {
                    added: vec![],
                    modified: vec![],
                    deleted: vec![],
                    current_sha,
                    previous_sha: None,
                    needs_full_survey: true,
                    full_survey_reason: Some("First survey of this repository".to_string()),
                });
            }
        };

        // Check if commit has changed
        if current_sha == repo_state.commit_sha {
            // No changes at all
            return Ok(ChangeResult {
                added: vec![],
                modified: vec![],
                deleted: vec![],
                current_sha,
                previous_sha: Some(repo_state.commit_sha.clone()),
                needs_full_survey: false,
                full_survey_reason: None,
            });
        }

        // Get changed files via git diff
        let changes = get_git_diff(&repo_state.commit_sha, &current_sha, repo_path).await?;

        Ok(ChangeResult {
            added: changes.added,
            modified: changes.modified,
            deleted: changes.deleted,
            current_sha,
            previous_sha: Some(repo_state.commit_sha.clone()),
            needs_full_survey: changes.needs_full_survey,
            full_survey_reason: changes.reason,
        })
    }
}

/// Get the current HEAD commit SHA for a repository.
pub async fn get_current_commit(repo_path: &Path) -> Result<String, ChangeError> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        return Err(ChangeError::GitError(
            "Failed to get HEAD commit. Is this a git repository?".to_string(),
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Internal struct for git diff results
struct GitDiffResult {
    added: Vec<PathBuf>,
    modified: Vec<PathBuf>,
    deleted: Vec<PathBuf>,
    needs_full_survey: bool,
    reason: Option<String>,
}

/// Get changed files between two commits using git diff.
async fn get_git_diff(
    from_sha: &str,
    to_sha: &str,
    repo_path: &Path,
) -> Result<GitDiffResult, ChangeError> {
    let output = Command::new("git")
        .args(["diff", "--name-status", from_sha, to_sha])
        .current_dir(repo_path)
        .output()
        .await?;

    if !output.status.success() {
        // Git diff failed - might be a force push, rebase, or shallow clone issue
        // Fall back to full survey
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Ok(GitDiffResult {
            added: vec![],
            modified: vec![],
            deleted: vec![],
            needs_full_survey: true,
            reason: Some(format!(
                "Git diff failed (possibly force push or shallow clone): {}",
                stderr.trim()
            )),
        });
    }

    let diff_str = String::from_utf8_lossy(&output.stdout);
    let mut result = GitDiffResult {
        added: vec![],
        modified: vec![],
        deleted: vec![],
        needs_full_survey: false,
        reason: None,
    };

    for line in diff_str.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let status = parts[0];
        let file_path = PathBuf::from(parts[1]);

        // Only track files we care about (parseable source files)
        if !is_parseable_file(&file_path) {
            continue;
        }

        match status {
            "A" => result.added.push(file_path),
            "M" => result.modified.push(file_path),
            "D" => result.deleted.push(file_path),
            s if s.starts_with('R') => {
                // Rename: R100 old_name new_name
                if parts.len() >= 3 {
                    let old_path = PathBuf::from(parts[1]);
                    let new_path = PathBuf::from(parts[2]);
                    if is_parseable_file(&old_path) {
                        result.deleted.push(old_path);
                    }
                    if is_parseable_file(&new_path) {
                        result.added.push(new_path);
                    }
                }
            }
            s if s.starts_with('C') => {
                // Copy: C100 source dest
                if parts.len() >= 3 {
                    let dest_path = PathBuf::from(parts[2]);
                    if is_parseable_file(&dest_path) {
                        result.added.push(dest_path);
                    }
                }
            }
            _ => {
                // Unknown status, treat modified files as modified
                result.modified.push(file_path);
            }
        }
    }

    Ok(result)
}

/// Check if a file should be parsed based on its extension.
///
/// Returns `true` for JavaScript, TypeScript, Python, and Terraform files.
pub fn is_parseable_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext.to_lowercase().as_str(),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "py" | "tf"
    )
}

/// Errors that can occur during incremental survey operations.
#[derive(Debug, Error)]
pub enum StateError {
    /// IO error during file operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Errors that can occur during change detection.
#[derive(Debug, Error)]
pub enum ChangeError {
    /// IO error during git operations
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Git command failed
    #[error("Git error: {0}")]
    GitError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_survey_state_new() {
        let state = SurveyState::new();
        assert_eq!(state.version, 1);
        assert!(state.repos.is_empty());
    }

    #[test]
    fn test_survey_state_mark_surveyed() {
        let mut state = SurveyState::new();
        state.mark_surveyed(
            "owner/repo",
            "abc123",
            42,
            vec!["javascript".to_string(), "typescript".to_string()],
            true,
        );

        assert_eq!(state.repo_count(), 1);
        assert_eq!(state.total_discoveries(), 42);

        let repo_state = state.get_repo("owner/repo").unwrap();
        assert_eq!(repo_state.commit_sha, "abc123");
        assert_eq!(repo_state.discovery_count, 42);
        assert!(repo_state.survey_successful);
        assert_eq!(repo_state.detected_languages.len(), 2);
    }

    #[test]
    fn test_survey_state_needs_survey() {
        let mut state = SurveyState::new();

        // New repo always needs survey
        assert!(state.needs_survey("owner/repo", "abc123"));

        // After marking surveyed with same SHA, doesn't need survey
        state.mark_surveyed("owner/repo", "abc123", 10, vec![], true);
        assert!(!state.needs_survey("owner/repo", "abc123"));

        // Different SHA needs survey
        assert!(state.needs_survey("owner/repo", "def456"));

        // Failed survey needs re-survey
        state.mark_surveyed("owner/repo2", "xyz789", 5, vec![], false);
        assert!(state.needs_survey("owner/repo2", "xyz789"));
    }

    #[test]
    fn test_survey_state_persistence() {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join("state.json");

        let mut state = SurveyState::new();
        state.mark_surveyed("owner/repo", "abc123", 42, vec!["rust".to_string()], true);

        state.save(&state_path).unwrap();

        let loaded = SurveyState::load(&state_path).unwrap();
        assert_eq!(loaded.repo_count(), 1);
        assert_eq!(loaded.total_discoveries(), 42);

        let repo = loaded.get_repo("owner/repo").unwrap();
        assert_eq!(repo.commit_sha, "abc123");
    }

    #[test]
    fn test_is_parseable_file() {
        // JavaScript/TypeScript
        assert!(is_parseable_file(Path::new("index.js")));
        assert!(is_parseable_file(Path::new("app.ts")));
        assert!(is_parseable_file(Path::new("component.tsx")));
        assert!(is_parseable_file(Path::new("component.jsx")));
        assert!(is_parseable_file(Path::new("module.mjs")));
        assert!(is_parseable_file(Path::new("module.cjs")));

        // Python
        assert!(is_parseable_file(Path::new("main.py")));
        assert!(is_parseable_file(Path::new("utils.PY"))); // Case insensitive

        // Terraform
        assert!(is_parseable_file(Path::new("main.tf")));

        // Non-parseable
        assert!(!is_parseable_file(Path::new("README.md")));
        assert!(!is_parseable_file(Path::new("package.json")));
        assert!(!is_parseable_file(Path::new("style.css")));
        assert!(!is_parseable_file(Path::new("no_extension")));
    }

    #[test]
    fn test_change_result() {
        let result = ChangeResult {
            added: vec![PathBuf::from("new.js")],
            modified: vec![PathBuf::from("changed.ts")],
            deleted: vec![PathBuf::from("removed.py")],
            current_sha: "abc123".to_string(),
            previous_sha: Some("def456".to_string()),
            needs_full_survey: false,
            full_survey_reason: None,
        };

        assert!(result.has_changes());
        assert_eq!(result.change_count(), 3);
        assert_eq!(result.files_to_parse().len(), 2);
    }

    #[test]
    fn test_change_result_no_changes() {
        let result = ChangeResult {
            added: vec![],
            modified: vec![],
            deleted: vec![],
            current_sha: "abc123".to_string(),
            previous_sha: Some("abc123".to_string()),
            needs_full_survey: false,
            full_survey_reason: None,
        };

        assert!(!result.has_changes());
        assert_eq!(result.change_count(), 0);
        assert!(result.files_to_parse().is_empty());
    }

    #[tokio::test]
    async fn test_get_current_commit_non_git_dir() {
        let dir = tempdir().unwrap();
        let result = get_current_commit(dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_current_commit_git_repo() {
        let dir = tempdir().unwrap();

        // Initialize git repo
        let init_output = Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        assert!(init_output.status.success());

        // Configure git user for commit
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        // Create a file and commit
        fs::write(dir.path().join("test.txt"), "hello").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        let sha = get_current_commit(dir.path()).await.unwrap();
        assert!(!sha.is_empty());
        assert_eq!(sha.len(), 40); // Git SHA is 40 hex chars
    }

    #[tokio::test]
    async fn test_change_detector_new_repo() {
        let state = SurveyState::new();
        let detector = ChangeDetector::new(state);

        let dir = tempdir().unwrap();

        // Initialize git repo with a commit
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        fs::write(dir.path().join("index.js"), "console.log('hello')").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        let result = detector
            .detect_changes("owner/repo", dir.path())
            .await
            .unwrap();

        assert!(result.needs_full_survey);
        assert!(result.previous_sha.is_none());
        assert_eq!(
            result.full_survey_reason,
            Some("First survey of this repository".to_string())
        );
    }

    #[tokio::test]
    async fn test_change_detector_no_changes() {
        let dir = tempdir().unwrap();

        // Initialize git repo with a commit
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        fs::write(dir.path().join("index.js"), "console.log('hello')").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        let sha = get_current_commit(dir.path()).await.unwrap();

        // Create state with the same SHA
        let mut state = SurveyState::new();
        state.mark_surveyed("owner/repo", &sha, 10, vec!["javascript".to_string()], true);

        let detector = ChangeDetector::new(state);
        let result = detector
            .detect_changes("owner/repo", dir.path())
            .await
            .unwrap();

        assert!(!result.needs_full_survey);
        assert!(!result.has_changes());
        assert_eq!(result.previous_sha, Some(sha));
    }

    #[tokio::test]
    async fn test_change_detector_with_changes() {
        let dir = tempdir().unwrap();

        // Initialize git repo with initial commit
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        fs::write(dir.path().join("index.js"), "console.log('hello')").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        let old_sha = get_current_commit(dir.path()).await.unwrap();

        // Make a change and commit
        fs::write(dir.path().join("new.ts"), "export const x = 1;").unwrap();
        fs::write(dir.path().join("index.js"), "console.log('modified')").unwrap();

        Command::new("git")
            .args(["add", "."])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        Command::new("git")
            .args(["commit", "-m", "changes"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();

        // Create state with old SHA
        let mut state = SurveyState::new();
        state.mark_surveyed(
            "owner/repo",
            &old_sha,
            10,
            vec!["javascript".to_string()],
            true,
        );

        let detector = ChangeDetector::new(state);
        let result = detector
            .detect_changes("owner/repo", dir.path())
            .await
            .unwrap();

        assert!(!result.needs_full_survey);
        assert!(result.has_changes());
        assert_eq!(result.added.len(), 1);
        assert_eq!(result.modified.len(), 1);
        assert!(result.added.contains(&PathBuf::from("new.ts")));
        assert!(result.modified.contains(&PathBuf::from("index.js")));
    }

    #[test]
    fn test_state_error_display() {
        let io_err = StateError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        assert!(io_err.to_string().contains("IO error"));

        let json_err =
            StateError::Json(serde_json::from_str::<SurveyState>("invalid").unwrap_err());
        assert!(json_err.to_string().contains("JSON error"));
    }

    #[test]
    fn test_change_error_display() {
        let git_err = ChangeError::GitError("not a git repo".to_string());
        assert!(git_err.to_string().contains("Git error"));
    }
}
