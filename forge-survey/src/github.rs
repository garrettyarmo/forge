//! GitHub API client for repository discovery and cloning.
//!
//! This module provides functionality for:
//! - Listing repositories in a GitHub organization
//! - Fetching information about specific repositories
//! - Cloning and caching repositories locally
//!
//! The client uses the `octocrab` library for GitHub API interactions
//! and shells out to `git` for clone/pull operations.

use octocrab::Octocrab;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::process::Command;

/// Errors that can occur during GitHub operations
#[derive(Debug, Error)]
pub enum GitHubError {
    /// GitHub API request failed
    #[error("GitHub API error: {0}")]
    ApiError(#[from] octocrab::Error),

    /// Git clone operation failed
    #[error("Git clone failed for {repo}: {message}")]
    CloneFailed {
        /// Repository that failed to clone
        repo: String,
        /// Error message
        message: String,
    },

    /// Git pull operation failed
    #[error("Git pull failed for {repo}: {message}")]
    PullFailed {
        /// Repository that failed to pull
        repo: String,
        /// Error message
        message: String,
    },

    /// Repository format is invalid
    #[error("Invalid repository format: {0}. Expected 'owner/repo'")]
    InvalidRepoFormat(String),

    /// Rate limit exceeded
    #[error("Rate limited. Retry after {0} seconds")]
    RateLimited(u64),

    /// Repository not found
    #[error("Repository not found: {0}")]
    RepoNotFound(String),

    /// Authentication failed
    #[error("Authentication failed. Check your GITHUB_TOKEN")]
    AuthFailed,

    /// IO error during file operations
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// Failed to build octocrab client
    #[error("Failed to build GitHub client: {0}")]
    ClientBuildError(String),
}

/// Information about a repository discovered from GitHub
#[derive(Debug, Clone)]
pub struct RepoInfo {
    /// Full name: "owner/repo"
    pub full_name: String,

    /// Just the repo name
    pub name: String,

    /// Owner/organization
    pub owner: String,

    /// Clone URL (https or ssh depending on configuration)
    pub clone_url: String,

    /// Default branch name
    pub default_branch: String,

    /// Primary language (as detected by GitHub)
    pub language: Option<String>,

    /// Whether the repository is archived
    pub archived: bool,

    /// Whether the repository is a fork
    pub fork: bool,

    /// Topics/tags associated with the repository
    pub topics: Vec<String>,
}

/// Git clone method to use
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloneMethod {
    /// Clone via HTTPS (default)
    #[default]
    Https,
    /// Clone via SSH
    Ssh,
}

/// GitHub API client wrapper
///
/// Provides methods for interacting with the GitHub API to discover
/// and fetch information about repositories.
pub struct GitHubClient {
    client: Octocrab,
    clone_method: CloneMethod,
}

impl GitHubClient {
    /// Create a new GitHub client with a personal access token
    ///
    /// # Arguments
    ///
    /// * `token` - GitHub personal access token
    /// * `api_url` - Optional base URL for GitHub Enterprise
    /// * `clone_method` - Whether to use HTTPS or SSH for cloning
    ///
    /// # Errors
    ///
    /// Returns an error if the client cannot be built
    pub fn new(
        token: &str,
        api_url: Option<&str>,
        clone_method: CloneMethod,
    ) -> Result<Self, GitHubError> {
        let mut builder = Octocrab::builder().personal_token(token.to_string());

        if let Some(url) = api_url {
            builder = builder
                .base_uri(url)
                .map_err(|e| GitHubError::ClientBuildError(e.to_string()))?;
        }

        let client = builder
            .build()
            .map_err(|e| GitHubError::ClientBuildError(e.to_string()))?;

        Ok(Self {
            client,
            clone_method,
        })
    }

    /// List all repositories in a GitHub organization
    ///
    /// Fetches all repositories with pagination, returning non-archived repos
    /// sorted by last update time.
    ///
    /// # Arguments
    ///
    /// * `org` - The organization name
    ///
    /// # Returns
    ///
    /// A vector of `RepoInfo` for all repositories in the organization
    pub async fn list_org_repos(&self, org: &str) -> Result<Vec<RepoInfo>, GitHubError> {
        let mut repos = Vec::new();
        let mut page = 1u32;

        loop {
            let page_repos = self
                .client
                .orgs(org)
                .list_repos()
                .repo_type(octocrab::params::repos::Type::All)
                .sort(octocrab::params::repos::Sort::Updated)
                .per_page(100)
                .page(page)
                .send()
                .await?;

            if page_repos.items.is_empty() {
                break;
            }

            for repo in page_repos.items {
                repos.push(self.repo_to_info(&repo, org));
            }

            page += 1;
        }

        Ok(repos)
    }

    /// Get information about a specific repository
    ///
    /// # Arguments
    ///
    /// * `owner` - The repository owner
    /// * `repo` - The repository name
    ///
    /// # Returns
    ///
    /// Repository information
    pub async fn get_repo(&self, owner: &str, repo: &str) -> Result<RepoInfo, GitHubError> {
        let repo_data = self.client.repos(owner, repo).get().await?;

        Ok(self.repo_to_info(&repo_data, owner))
    }

    /// Convert an octocrab Repository to our RepoInfo
    fn repo_to_info(&self, repo: &octocrab::models::Repository, owner: &str) -> RepoInfo {
        RepoInfo {
            full_name: repo
                .full_name
                .clone()
                .unwrap_or_else(|| format!("{}/{}", owner, repo.name)),
            name: repo.name.clone(),
            owner: owner.to_string(),
            clone_url: self.get_clone_url(repo),
            default_branch: repo
                .default_branch
                .clone()
                .unwrap_or_else(|| "main".to_string()),
            // Convert Value to String - language can be a JSON string or null
            language: repo
                .language
                .as_ref()
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            archived: repo.archived.unwrap_or(false),
            fork: repo.fork.unwrap_or(false),
            topics: repo.topics.clone().unwrap_or_default(),
        }
    }

    /// Get the appropriate clone URL based on the configured clone method
    fn get_clone_url(&self, repo: &octocrab::models::Repository) -> String {
        match self.clone_method {
            CloneMethod::Https => repo
                .clone_url
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default(),
            CloneMethod::Ssh => repo.ssh_url.clone().unwrap_or_default(),
        }
    }
}

/// Manages local repository cache for cloned repositories
///
/// Handles cloning new repositories and updating existing ones.
/// Repositories are organized by owner/name in the cache directory.
pub struct RepoCache {
    cache_dir: PathBuf,
    clone_method: CloneMethod,
}

impl RepoCache {
    /// Create a new repository cache manager
    ///
    /// # Arguments
    ///
    /// * `cache_dir` - Directory where repositories will be cloned
    /// * `clone_method` - Whether to use HTTPS or SSH for cloning
    pub fn new(cache_dir: PathBuf, clone_method: CloneMethod) -> Self {
        Self {
            cache_dir,
            clone_method,
        }
    }

    /// Get the local path where a repository would be stored
    ///
    /// # Arguments
    ///
    /// * `repo` - Repository information
    ///
    /// # Returns
    ///
    /// The path where the repository is/would be cloned
    pub fn repo_path(&self, repo: &RepoInfo) -> PathBuf {
        self.cache_dir.join(&repo.owner).join(&repo.name)
    }

    /// Ensure a repository is cloned and up to date
    ///
    /// If the repository doesn't exist locally, it will be cloned.
    /// If it exists, it will be updated with the latest changes.
    ///
    /// # Arguments
    ///
    /// * `repo` - Repository information
    /// * `token` - Optional GitHub token for HTTPS authentication
    ///
    /// # Returns
    ///
    /// The local path to the repository
    pub async fn ensure_repo(
        &self,
        repo: &RepoInfo,
        token: Option<&str>,
    ) -> Result<PathBuf, GitHubError> {
        let local_path = self.repo_path(repo);

        if local_path.exists() {
            // Pull latest changes
            self.pull_repo(&local_path, repo).await?;
        } else {
            // Clone the repository
            self.clone_repo(repo, &local_path, token).await?;
        }

        Ok(local_path)
    }

    /// Clone a repository to a local path
    async fn clone_repo(
        &self,
        repo: &RepoInfo,
        local_path: &Path,
        token: Option<&str>,
    ) -> Result<(), GitHubError> {
        // Create parent directories
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| GitHubError::CloneFailed {
                    repo: repo.full_name.clone(),
                    message: format!("Failed to create directory: {}", e),
                })?;
        }

        // Build clone URL with token for HTTPS authentication
        let clone_url = match (self.clone_method, token) {
            (CloneMethod::Https, Some(token)) => {
                // Insert token into URL: https://TOKEN@github.com/owner/repo.git
                repo.clone_url
                    .replace("https://", &format!("https://{}@", token))
            }
            _ => repo.clone_url.clone(),
        };

        tracing::info!("Cloning {} to {}", repo.full_name, local_path.display());

        // Execute git clone with shallow clone for speed
        let output = Command::new("git")
            .args([
                "clone",
                "--depth",
                "1", // Shallow clone for speed
                "--single-branch",
                "--branch",
                &repo.default_branch,
                &clone_url,
                local_path.to_str().unwrap_or_default(),
            ])
            .output()
            .await
            .map_err(|e| GitHubError::CloneFailed {
                repo: repo.full_name.clone(),
                message: e.to_string(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitHubError::CloneFailed {
                repo: repo.full_name.clone(),
                message: stderr.to_string(),
            });
        }

        tracing::info!("Successfully cloned {}", repo.full_name);
        Ok(())
    }

    /// Pull latest changes for an existing repository
    async fn pull_repo(&self, local_path: &Path, repo: &RepoInfo) -> Result<(), GitHubError> {
        tracing::debug!("Pulling updates for {}", repo.full_name);

        let output = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(local_path)
            .output()
            .await
            .map_err(|e| GitHubError::PullFailed {
                repo: repo.full_name.clone(),
                message: e.to_string(),
            })?;

        if !output.status.success() {
            // Pull failed, might be dirty or diverged - try to reset to origin
            tracing::warn!(
                "Pull failed for {}, attempting reset to origin",
                repo.full_name
            );

            let _ = Command::new("git")
                .args(["fetch", "origin"])
                .current_dir(local_path)
                .output()
                .await;

            let reset_output = Command::new("git")
                .args([
                    "reset",
                    "--hard",
                    &format!("origin/{}", repo.default_branch),
                ])
                .current_dir(local_path)
                .output()
                .await;

            if reset_output.is_err() {
                tracing::warn!(
                    "Failed to reset {} to origin, continuing with existing state",
                    repo.full_name
                );
            }
        }

        Ok(())
    }

    /// Get the current commit SHA of a repository
    ///
    /// # Arguments
    ///
    /// * `local_path` - Path to the local repository
    ///
    /// # Returns
    ///
    /// The commit SHA if it can be determined, None otherwise
    pub async fn get_commit_sha(&self, local_path: &Path) -> Option<String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(local_path)
            .output()
            .await
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    /// Check if a repository exists in the cache
    ///
    /// # Arguments
    ///
    /// * `repo` - Repository information
    ///
    /// # Returns
    ///
    /// True if the repository exists locally
    pub fn repo_exists(&self, repo: &RepoInfo) -> bool {
        self.repo_path(repo).exists()
    }

    /// Get the cache directory path
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

/// Parse a repository string in "owner/repo" format
///
/// # Arguments
///
/// * `repo_str` - Repository string in "owner/repo" format
///
/// # Returns
///
/// A tuple of (owner, repo) or an error if the format is invalid
pub fn parse_repo_string(repo_str: &str) -> Result<(&str, &str), GitHubError> {
    let parts: Vec<&str> = repo_str.split('/').collect();
    if parts.len() != 2 {
        return Err(GitHubError::InvalidRepoFormat(repo_str.to_string()));
    }
    Ok((parts[0], parts[1]))
}

/// Create a RepoInfo from an owner/repo string without API calls
///
/// This is useful when you have a list of repos but don't need
/// all the metadata that comes from the API.
///
/// # Arguments
///
/// * `owner` - Repository owner
/// * `name` - Repository name
/// * `clone_method` - Clone method to use
///
/// # Returns
///
/// A RepoInfo with default values for unknown fields
pub fn create_repo_info_minimal(owner: &str, name: &str, clone_method: CloneMethod) -> RepoInfo {
    let clone_url = match clone_method {
        CloneMethod::Https => format!("https://github.com/{}/{}.git", owner, name),
        CloneMethod::Ssh => format!("git@github.com:{}/{}.git", owner, name),
    };

    RepoInfo {
        full_name: format!("{}/{}", owner, name),
        name: name.to_string(),
        owner: owner.to_string(),
        clone_url,
        default_branch: "main".to_string(),
        language: None,
        archived: false,
        fork: false,
        topics: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_repo_string_valid() {
        let (owner, repo) = parse_repo_string("octocat/hello-world").unwrap();
        assert_eq!(owner, "octocat");
        assert_eq!(repo, "hello-world");
    }

    #[test]
    fn test_parse_repo_string_invalid_no_slash() {
        let result = parse_repo_string("invalid");
        assert!(matches!(result, Err(GitHubError::InvalidRepoFormat(_))));
    }

    #[test]
    fn test_parse_repo_string_invalid_too_many_slashes() {
        let result = parse_repo_string("too/many/slashes");
        assert!(matches!(result, Err(GitHubError::InvalidRepoFormat(_))));
    }

    #[test]
    fn test_create_repo_info_minimal_https() {
        let info = create_repo_info_minimal("owner", "repo", CloneMethod::Https);
        assert_eq!(info.full_name, "owner/repo");
        assert_eq!(info.name, "repo");
        assert_eq!(info.owner, "owner");
        assert_eq!(info.clone_url, "https://github.com/owner/repo.git");
        assert_eq!(info.default_branch, "main");
        assert!(!info.archived);
        assert!(!info.fork);
    }

    #[test]
    fn test_create_repo_info_minimal_ssh() {
        let info = create_repo_info_minimal("owner", "repo", CloneMethod::Ssh);
        assert_eq!(info.clone_url, "git@github.com:owner/repo.git");
    }

    #[test]
    fn test_repo_cache_path() {
        let cache = RepoCache::new(PathBuf::from("/tmp/forge/repos"), CloneMethod::Https);
        let repo = create_repo_info_minimal("myorg", "myrepo", CloneMethod::Https);
        let path = cache.repo_path(&repo);
        assert_eq!(path, PathBuf::from("/tmp/forge/repos/myorg/myrepo"));
    }

    #[test]
    fn test_clone_method_default() {
        let method = CloneMethod::default();
        assert_eq!(method, CloneMethod::Https);
    }
}
