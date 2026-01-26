//! Language detection module for Forge survey.
//!
//! This module provides functionality to automatically detect programming languages
//! used in a repository based on file extensions and configuration files.
//!
//! # Detection Strategy
//!
//! Languages are detected using two complementary methods:
//!
//! 1. **File Extension Scanning**: Counts files with known extensions (`.js`, `.py`, etc.)
//!    and requires a minimum threshold (≥3 files) for detection. Confidence: 0.7
//!
//! 2. **Configuration File Detection**: Looks for language-specific config files
//!    (e.g., `package.json`, `pyproject.toml`). Confidence: 0.95
//!
//! The module scans the repository root and common source directories (`src/`, `lib/`, `app/`)
//! with a maximum depth of 3 to balance thoroughness with performance.
//!
//! # Supported Languages
//!
//! - **JavaScript**: `.js`, `.jsx`, `.mjs`, `.cjs` or `package.json`
//! - **TypeScript**: `.ts`, `.tsx` or `package.json` with TypeScript dependencies
//! - **Python**: `.py` or `requirements.txt`, `pyproject.toml`, `setup.py`, `setup.cfg`, `Pipfile`
//! - **Terraform**: `.tf`, `.tfvars`

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use walkdir::WalkDir;

/// Minimum number of files required for extension-based language detection.
const FILE_THRESHOLD: usize = 3;

/// Confidence score for extension-based detection.
const EXTENSION_CONFIDENCE: f64 = 0.7;

/// Confidence score for config file-based detection.
const CONFIG_CONFIDENCE: f64 = 0.95;

/// Maximum directory depth to scan.
const MAX_DEPTH: usize = 3;

/// Directories to scan for source files (relative to repo root).
const SCAN_DIRECTORIES: &[&str] = &["", "src", "lib", "app"];

/// Represents a detected programming language with its confidence score.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedLanguage {
    /// The name of the detected language (e.g., "javascript", "python").
    pub name: String,

    /// Confidence score between 0.0 and 1.0.
    /// Higher scores indicate more certain detection.
    pub confidence: f64,

    /// How the language was detected.
    pub detection_method: DetectionMethod,
}

/// How a language was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMethod {
    /// Detected by counting file extensions.
    FileExtension,

    /// Detected by finding a configuration file.
    ConfigFile,
}

/// Collection of detected languages in a repository.
#[derive(Debug, Clone, Default)]
pub struct DetectedLanguages {
    /// Map from language name to detection info.
    /// Using a map ensures uniqueness by language name.
    languages: HashMap<String, DetectedLanguage>,
}

impl DetectedLanguages {
    /// Creates a new empty DetectedLanguages instance.
    pub fn new() -> Self {
        Self {
            languages: HashMap::new(),
        }
    }

    /// Adds a detected language, keeping the higher confidence if already present.
    ///
    /// If the language is already in the set, only updates if the new confidence
    /// is higher than the existing one.
    pub fn add(&mut self, language: DetectedLanguage) {
        let name = language.name.clone();
        match self.languages.get(&name) {
            Some(existing) if existing.confidence >= language.confidence => {
                // Keep existing higher-confidence detection
            }
            _ => {
                self.languages.insert(name, language);
            }
        }
    }

    /// Returns all detected languages as a slice.
    pub fn languages(&self) -> Vec<&DetectedLanguage> {
        self.languages.values().collect()
    }

    /// Returns the number of detected languages.
    pub fn len(&self) -> usize {
        self.languages.len()
    }

    /// Returns true if no languages were detected.
    pub fn is_empty(&self) -> bool {
        self.languages.is_empty()
    }

    /// Checks if a specific language was detected.
    pub fn contains(&self, language: &str) -> bool {
        self.languages.contains_key(language)
    }

    /// Gets the detection info for a specific language.
    pub fn get(&self, language: &str) -> Option<&DetectedLanguage> {
        self.languages.get(language)
    }

    /// Returns an iterator over the detected languages.
    pub fn iter(&self) -> impl Iterator<Item = &DetectedLanguage> {
        self.languages.values()
    }
}

/// Main entry point for language detection.
///
/// Detects programming languages used in a repository by scanning for
/// file extensions and configuration files.
///
/// # Arguments
///
/// * `repo_path` - Path to the repository root directory
///
/// # Returns
///
/// A `DetectedLanguages` struct containing all detected languages with
/// their confidence scores. Languages are deduplicated, with higher
/// confidence detections taking precedence.
///
/// # Example
///
/// ```ignore
/// use std::path::Path;
/// use forge_survey::detection::detect_languages;
///
/// let detected = detect_languages(Path::new("/path/to/repo"));
/// for lang in detected.iter() {
///     println!("{}: {:.0}% confidence", lang.name, lang.confidence * 100.0);
/// }
/// ```
pub fn detect_languages(repo_path: &Path) -> DetectedLanguages {
    let mut detected = DetectedLanguages::new();

    // Check config files first (higher confidence)
    let config_languages = check_config_files(repo_path);
    for lang in config_languages {
        detected.add(lang);
    }

    // Then scan file extensions
    let extension_languages = scan_file_extensions(repo_path);
    for lang in extension_languages {
        detected.add(lang);
    }

    detected
}

/// Scans the repository for file extensions to detect languages.
///
/// Counts files with known extensions in the repo root and common source
/// directories. A language is detected if at least `FILE_THRESHOLD` (3) files
/// with matching extensions are found.
///
/// # Arguments
///
/// * `repo_path` - Path to the repository root directory
///
/// # Returns
///
/// A vector of detected languages based on file extensions.
/// Each detection has a confidence score of 0.7.
pub fn scan_file_extensions(repo_path: &Path) -> Vec<DetectedLanguage> {
    let mut extension_counts: HashMap<&str, usize> = HashMap::new();

    // Scan each directory
    for dir in SCAN_DIRECTORIES {
        let scan_path = if dir.is_empty() {
            repo_path.to_path_buf()
        } else {
            repo_path.join(dir)
        };

        if !scan_path.exists() || !scan_path.is_dir() {
            continue;
        }

        // Walk directory with depth limit
        for entry in WalkDir::new(&scan_path)
            .max_depth(MAX_DEPTH)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_ignored_dir(e.file_name().to_str().unwrap_or("")))
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }

            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                // Count known extensions
                match ext {
                    // JavaScript extensions
                    "js" | "jsx" | "mjs" | "cjs" => {
                        *extension_counts.entry("javascript").or_insert(0) += 1;
                    }
                    // TypeScript extensions
                    "ts" | "tsx" => {
                        *extension_counts.entry("typescript").or_insert(0) += 1;
                    }
                    // Python extensions
                    "py" => {
                        *extension_counts.entry("python").or_insert(0) += 1;
                    }
                    // Terraform extensions
                    "tf" | "tfvars" => {
                        *extension_counts.entry("terraform").or_insert(0) += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    // Convert counts to detections if threshold met
    extension_counts
        .into_iter()
        .filter(|(_, count)| *count >= FILE_THRESHOLD)
        .map(|(lang, _)| DetectedLanguage {
            name: lang.to_string(),
            confidence: EXTENSION_CONFIDENCE,
            detection_method: DetectionMethod::FileExtension,
        })
        .collect()
}

/// Checks for language-specific configuration files.
///
/// Looks for config files in the repository root that indicate the presence
/// of specific languages. Config file detection has higher confidence (0.95)
/// than extension scanning.
///
/// # Detected Config Files
///
/// - **JavaScript**: `package.json` (without TypeScript dependencies)
/// - **TypeScript**: `package.json` with "typescript" or "ts-" prefixed dependencies
/// - **Python**: `requirements.txt`, `pyproject.toml`, `setup.py`, `setup.cfg`, `Pipfile`
/// - **Terraform**: `.tf` files (no specific config, relies on extension scanning)
///
/// # Arguments
///
/// * `repo_path` - Path to the repository root directory
///
/// # Returns
///
/// A vector of detected languages based on configuration files.
/// Each detection has a confidence score of 0.95.
pub fn check_config_files(repo_path: &Path) -> Vec<DetectedLanguage> {
    let mut detected = Vec::new();
    let mut detected_names: HashSet<String> = HashSet::new();

    // Check for package.json (JavaScript/TypeScript)
    let package_json_path = repo_path.join("package.json");
    if package_json_path.exists() {
        if let Ok(content) = fs::read_to_string(&package_json_path) {
            // Check if TypeScript is present
            let has_typescript = content.contains("\"typescript\"")
                || content.contains("\"ts-node\"")
                || content.contains("\"ts-jest\"")
                || content.contains("\"ts-loader\"");

            if has_typescript {
                detected.push(DetectedLanguage {
                    name: "typescript".to_string(),
                    confidence: CONFIG_CONFIDENCE,
                    detection_method: DetectionMethod::ConfigFile,
                });
                detected_names.insert("typescript".to_string());
            }

            // Always detect JavaScript if package.json exists
            // (TypeScript projects also use JavaScript tooling)
            detected.push(DetectedLanguage {
                name: "javascript".to_string(),
                confidence: CONFIG_CONFIDENCE,
                detection_method: DetectionMethod::ConfigFile,
            });
            detected_names.insert("javascript".to_string());
        }
    }

    // Check for Python config files
    let python_configs = [
        "requirements.txt",
        "pyproject.toml",
        "setup.py",
        "setup.cfg",
        "Pipfile",
    ];

    for config in &python_configs {
        if !detected_names.contains("python") && repo_path.join(config).exists() {
            detected.push(DetectedLanguage {
                name: "python".to_string(),
                confidence: CONFIG_CONFIDENCE,
                detection_method: DetectionMethod::ConfigFile,
            });
            detected_names.insert("python".to_string());
            break; // Only add Python once
        }
    }

    // Note: Terraform doesn't have a specific config file
    // It relies on extension scanning (.tf, .tfvars)

    // Check for CloudFormation/SAM template files
    let cloudformation_templates = [
        "template.yaml",
        "template.yml",
        "template.json",
        "samconfig.yaml",
        "samconfig.yml",
        "samconfig.toml",
    ];

    for template in &cloudformation_templates {
        if !detected_names.contains("cloudformation") && repo_path.join(template).exists() {
            // Check if it's actually a CloudFormation/SAM template by reading the file
            let template_path = repo_path.join(template);
            if let Ok(content) = fs::read_to_string(&template_path) {
                // Check for CloudFormation markers
                if content.contains("AWSTemplateFormatVersion")
                    || content.contains("Transform")
                    || content.contains("AWS::Serverless")
                    || content.contains("AWS::Lambda")
                    || content.contains("AWS::DynamoDB")
                {
                    detected.push(DetectedLanguage {
                        name: "cloudformation".to_string(),
                        confidence: CONFIG_CONFIDENCE,
                        detection_method: DetectionMethod::ConfigFile,
                    });
                    detected_names.insert("cloudformation".to_string());
                    break;
                }
            }
        }
    }

    detected
}

/// Checks if a directory should be ignored during scanning.
///
/// Skips common directories that don't contain relevant source code
/// or would slow down scanning (e.g., `node_modules`, `.git`, `target`).
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        // JavaScript/Node.js
        "node_modules"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | "coverage"
            | ".turbo"
            | ".parcel-cache"
            // Python
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | "venv"
            | ".venv"
            | "env"
            | ".tox"
            | ".nox"
            // Rust
            | "target"
            // General
            | ".git"
            | ".svn"
            | ".hg"
            | "vendor"
            | ".idea"
            | ".vscode"
            | ".github"
            // Build outputs
            | "out"
            | "output"
            | "bin"
            | "obj"
            // Terraform
            | ".terraform"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    /// Helper to create a temporary directory with specified files.
    fn create_test_repo(files: &[&str]) -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        for file_path in files {
            let full_path = temp_dir.path().join(file_path);
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            File::create(&full_path).unwrap();
        }
        temp_dir
    }

    /// Helper to create a file with specific content.
    fn create_file_with_content(dir: &Path, file_path: &str, content: &str) {
        let full_path = dir.join(file_path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut file = File::create(&full_path).unwrap();
        write!(file, "{}", content).unwrap();
    }

    // ==================== DetectedLanguages Tests ====================

    #[test]
    fn test_detected_languages_new() {
        let detected = DetectedLanguages::new();
        assert!(detected.is_empty());
        assert_eq!(detected.len(), 0);
    }

    #[test]
    fn test_detected_languages_add_single() {
        let mut detected = DetectedLanguages::new();
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        });

        assert_eq!(detected.len(), 1);
        assert!(detected.contains("javascript"));
        assert!(!detected.contains("python"));
    }

    #[test]
    fn test_detected_languages_add_keeps_higher_confidence() {
        let mut detected = DetectedLanguages::new();

        // Add with lower confidence first
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        });

        // Add with higher confidence
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.95,
            detection_method: DetectionMethod::ConfigFile,
        });

        // Should keep the higher confidence
        let js = detected.get("javascript").unwrap();
        assert_eq!(js.confidence, 0.95);
        assert_eq!(js.detection_method, DetectionMethod::ConfigFile);
    }

    #[test]
    fn test_detected_languages_add_ignores_lower_confidence() {
        let mut detected = DetectedLanguages::new();

        // Add with higher confidence first
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.95,
            detection_method: DetectionMethod::ConfigFile,
        });

        // Try to add with lower confidence
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        });

        // Should keep the original higher confidence
        let js = detected.get("javascript").unwrap();
        assert_eq!(js.confidence, 0.95);
        assert_eq!(js.detection_method, DetectionMethod::ConfigFile);
    }

    #[test]
    fn test_detected_languages_multiple() {
        let mut detected = DetectedLanguages::new();
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        });
        detected.add(DetectedLanguage {
            name: "python".to_string(),
            confidence: 0.95,
            detection_method: DetectionMethod::ConfigFile,
        });

        assert_eq!(detected.len(), 2);
        assert!(detected.contains("javascript"));
        assert!(detected.contains("python"));
    }

    #[test]
    fn test_detected_languages_iter() {
        let mut detected = DetectedLanguages::new();
        detected.add(DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        });
        detected.add(DetectedLanguage {
            name: "python".to_string(),
            confidence: 0.95,
            detection_method: DetectionMethod::ConfigFile,
        });

        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("javascript"));
        assert!(names.contains("python"));
    }

    // ==================== scan_file_extensions Tests ====================

    #[test]
    fn test_scan_extensions_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let detected = scan_file_extensions(temp_dir.path());
        assert!(detected.is_empty());
    }

    #[test]
    fn test_scan_extensions_below_threshold() {
        // Create only 2 JS files (below threshold of 3)
        let temp_dir = create_test_repo(&["file1.js", "file2.js"]);
        let detected = scan_file_extensions(temp_dir.path());
        assert!(detected.is_empty());
    }

    #[test]
    fn test_scan_extensions_javascript_at_threshold() {
        // Create exactly 3 JS files (at threshold)
        let temp_dir = create_test_repo(&["file1.js", "file2.js", "file3.js"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
        assert_eq!(detected[0].confidence, EXTENSION_CONFIDENCE);
        assert_eq!(detected[0].detection_method, DetectionMethod::FileExtension);
    }

    #[test]
    fn test_scan_extensions_javascript_variants() {
        // Test all JavaScript variants count toward same language
        let temp_dir = create_test_repo(&["file.js", "component.jsx", "module.mjs", "config.cjs"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
    }

    #[test]
    fn test_scan_extensions_typescript() {
        let temp_dir = create_test_repo(&["file1.ts", "file2.ts", "component.tsx"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "typescript");
    }

    #[test]
    fn test_scan_extensions_python() {
        let temp_dir = create_test_repo(&["main.py", "utils.py", "test_main.py"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
    }

    #[test]
    fn test_scan_extensions_terraform() {
        let temp_dir = create_test_repo(&["main.tf", "variables.tf", "prod.tfvars"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "terraform");
    }

    #[test]
    fn test_scan_extensions_multiple_languages() {
        let temp_dir = create_test_repo(&[
            // JavaScript (3 files)
            "file1.js",
            "file2.js",
            "file3.js",
            // Python (3 files)
            "main.py",
            "utils.py",
            "test_main.py",
        ]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 2);
        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("javascript"));
        assert!(names.contains("python"));
    }

    #[test]
    fn test_scan_extensions_in_src_directory() {
        // Files in src/ should be counted
        let temp_dir = create_test_repo(&["src/file1.js", "src/file2.js", "src/file3.js"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
    }

    #[test]
    fn test_scan_extensions_in_lib_directory() {
        // Files in lib/ should be counted
        let temp_dir = create_test_repo(&["lib/file1.py", "lib/file2.py", "lib/file3.py"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
    }

    #[test]
    fn test_scan_extensions_in_app_directory() {
        // Files in app/ should be counted
        let temp_dir = create_test_repo(&["app/file1.ts", "app/file2.ts", "app/file3.ts"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "typescript");
    }

    #[test]
    fn test_scan_extensions_ignores_node_modules() {
        let temp_dir =
            create_test_repo(&["node_modules/pkg1/file.js", "node_modules/pkg2/index.js"]);
        // Add some files outside node_modules
        create_file_with_content(temp_dir.path(), "src/app.js", "");

        let detected = scan_file_extensions(temp_dir.path());

        // Should not detect javascript since only 1 file outside node_modules
        assert!(detected.is_empty());
    }

    #[test]
    fn test_scan_extensions_ignores_pycache() {
        let temp_dir = create_test_repo(&[
            "__pycache__/file1.py",
            "__pycache__/file2.py",
            "__pycache__/file3.py",
        ]);

        let detected = scan_file_extensions(temp_dir.path());
        assert!(detected.is_empty());
    }

    #[test]
    fn test_scan_extensions_combines_directories() {
        // Files split across root and src should combine
        let temp_dir = create_test_repo(&["file1.js", "src/file2.js", "lib/file3.js"]);
        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
    }

    // ==================== check_config_files Tests ====================

    #[test]
    fn test_config_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let detected = check_config_files(temp_dir.path());
        assert!(detected.is_empty());
    }

    #[test]
    fn test_config_package_json_javascript() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{
                "name": "my-app",
                "dependencies": {
                    "express": "^4.18.0"
                }
            }"#,
        );

        let detected = check_config_files(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
        assert_eq!(detected[0].confidence, CONFIG_CONFIDENCE);
        assert_eq!(detected[0].detection_method, DetectionMethod::ConfigFile);
    }

    #[test]
    fn test_config_package_json_typescript() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{
                "name": "my-app",
                "devDependencies": {
                    "typescript": "^5.0.0"
                }
            }"#,
        );

        let detected = check_config_files(temp_dir.path());

        // Should detect both TypeScript and JavaScript
        assert_eq!(detected.len(), 2);
        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("typescript"));
        assert!(names.contains("javascript"));
    }

    #[test]
    fn test_config_package_json_ts_node() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{
                "devDependencies": {
                    "ts-node": "^10.0.0"
                }
            }"#,
        );

        let detected = check_config_files(temp_dir.path());

        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("typescript"));
    }

    #[test]
    fn test_config_package_json_ts_jest() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{
                "devDependencies": {
                    "ts-jest": "^29.0.0"
                }
            }"#,
        );

        let detected = check_config_files(temp_dir.path());

        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("typescript"));
    }

    #[test]
    fn test_config_package_json_ts_loader() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{
                "devDependencies": {
                    "ts-loader": "^9.0.0"
                }
            }"#,
        );

        let detected = check_config_files(temp_dir.path());

        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("typescript"));
    }

    #[test]
    fn test_config_requirements_txt() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "requirements.txt",
            "flask==2.0.0\nrequests",
        );

        let detected = check_config_files(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
        assert_eq!(detected[0].confidence, CONFIG_CONFIDENCE);
    }

    #[test]
    fn test_config_pyproject_toml() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "pyproject.toml",
            "[project]\nname = \"my-app\"",
        );

        let detected = check_config_files(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
    }

    #[test]
    fn test_config_setup_py() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "setup.py",
            "from setuptools import setup\nsetup()",
        );

        let detected = check_config_files(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
    }

    #[test]
    fn test_config_setup_cfg() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(temp_dir.path(), "setup.cfg", "[metadata]\nname = my-app");

        let detected = check_config_files(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
    }

    #[test]
    fn test_config_pipfile() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "Pipfile",
            "[[source]]\nurl = \"https://pypi.org/simple\"",
        );

        let detected = check_config_files(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "python");
    }

    #[test]
    fn test_config_multiple_python_files_only_one_detection() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(temp_dir.path(), "requirements.txt", "flask");
        create_file_with_content(
            temp_dir.path(),
            "pyproject.toml",
            "[project]\nname = \"app\"",
        );
        create_file_with_content(temp_dir.path(), "setup.py", "setup()");

        let detected = check_config_files(temp_dir.path());

        // Should only have one Python entry despite multiple config files
        let python_count = detected.iter().filter(|l| l.name == "python").count();
        assert_eq!(python_count, 1);
    }

    #[test]
    fn test_config_multiple_languages() {
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{"dependencies": {"express": "^4.0.0"}}"#,
        );
        create_file_with_content(temp_dir.path(), "requirements.txt", "flask");

        let detected = check_config_files(temp_dir.path());

        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("javascript"));
        assert!(names.contains("python"));
    }

    // ==================== detect_languages Integration Tests ====================

    #[test]
    fn test_detect_languages_empty_repo() {
        let temp_dir = TempDir::new().unwrap();
        let detected = detect_languages(temp_dir.path());
        assert!(detected.is_empty());
    }

    #[test]
    fn test_detect_languages_config_takes_precedence() {
        let temp_dir = TempDir::new().unwrap();

        // Create package.json (config detection)
        create_file_with_content(temp_dir.path(), "package.json", r#"{"name": "app"}"#);

        // Also create JS files (extension detection)
        create_file_with_content(temp_dir.path(), "file1.js", "");
        create_file_with_content(temp_dir.path(), "file2.js", "");
        create_file_with_content(temp_dir.path(), "file3.js", "");

        let detected = detect_languages(temp_dir.path());

        // Should have JavaScript with config confidence (higher)
        assert!(detected.contains("javascript"));
        let js = detected.get("javascript").unwrap();
        assert_eq!(js.confidence, CONFIG_CONFIDENCE);
        assert_eq!(js.detection_method, DetectionMethod::ConfigFile);
    }

    #[test]
    fn test_detect_languages_extension_only() {
        let temp_dir = create_test_repo(&["file1.py", "file2.py", "file3.py"]);

        let detected = detect_languages(temp_dir.path());

        assert!(detected.contains("python"));
        let py = detected.get("python").unwrap();
        assert_eq!(py.confidence, EXTENSION_CONFIDENCE);
        assert_eq!(py.detection_method, DetectionMethod::FileExtension);
    }

    #[test]
    fn test_detect_languages_mixed_sources() {
        let temp_dir = TempDir::new().unwrap();

        // JavaScript via config
        create_file_with_content(temp_dir.path(), "package.json", r#"{"name": "app"}"#);

        // Terraform via extension only
        create_file_with_content(temp_dir.path(), "main.tf", "");
        create_file_with_content(temp_dir.path(), "variables.tf", "");
        create_file_with_content(temp_dir.path(), "outputs.tf", "");

        let detected = detect_languages(temp_dir.path());

        assert!(detected.contains("javascript"));
        assert!(detected.contains("terraform"));

        // JavaScript should have config confidence
        assert_eq!(
            detected.get("javascript").unwrap().confidence,
            CONFIG_CONFIDENCE
        );

        // Terraform should have extension confidence
        assert_eq!(
            detected.get("terraform").unwrap().confidence,
            EXTENSION_CONFIDENCE
        );
    }

    #[test]
    fn test_detect_languages_full_stack_project() {
        let temp_dir = TempDir::new().unwrap();

        // TypeScript frontend
        create_file_with_content(
            temp_dir.path(),
            "package.json",
            r#"{
                "devDependencies": {
                    "typescript": "^5.0.0"
                }
            }"#,
        );

        // Python backend
        create_file_with_content(temp_dir.path(), "requirements.txt", "fastapi");

        // Terraform infrastructure
        create_file_with_content(temp_dir.path(), "infra/main.tf", "");
        create_file_with_content(temp_dir.path(), "infra/variables.tf", "");
        create_file_with_content(temp_dir.path(), "infra/outputs.tf", "");

        let detected = detect_languages(temp_dir.path());

        assert!(detected.contains("javascript"));
        assert!(detected.contains("typescript"));
        assert!(detected.contains("python"));
        assert!(detected.contains("terraform"));
    }

    #[test]
    fn test_detect_languages_no_duplicates() {
        let temp_dir = TempDir::new().unwrap();

        // Multiple Python configs
        create_file_with_content(temp_dir.path(), "requirements.txt", "flask");
        create_file_with_content(temp_dir.path(), "pyproject.toml", "[project]");

        // Plus Python files
        create_file_with_content(temp_dir.path(), "main.py", "");
        create_file_with_content(temp_dir.path(), "utils.py", "");
        create_file_with_content(temp_dir.path(), "test.py", "");

        let detected = detect_languages(temp_dir.path());

        // Should only have one Python entry
        let python_entries: Vec<_> = detected
            .languages()
            .into_iter()
            .filter(|l| l.name == "python")
            .collect();
        assert_eq!(python_entries.len(), 1);

        // And it should have config confidence
        assert_eq!(python_entries[0].confidence, CONFIG_CONFIDENCE);
    }

    // ==================== is_ignored_dir Tests ====================

    #[test]
    fn test_is_ignored_dir_node_modules() {
        assert!(is_ignored_dir("node_modules"));
    }

    #[test]
    fn test_is_ignored_dir_git() {
        assert!(is_ignored_dir(".git"));
    }

    #[test]
    fn test_is_ignored_dir_pycache() {
        assert!(is_ignored_dir("__pycache__"));
    }

    #[test]
    fn test_is_ignored_dir_venv() {
        assert!(is_ignored_dir("venv"));
        assert!(is_ignored_dir(".venv"));
    }

    #[test]
    fn test_is_ignored_dir_target() {
        assert!(is_ignored_dir("target"));
    }

    #[test]
    fn test_is_ignored_dir_terraform() {
        assert!(is_ignored_dir(".terraform"));
    }

    #[test]
    fn test_is_ignored_dir_source_dirs_not_ignored() {
        assert!(!is_ignored_dir("src"));
        assert!(!is_ignored_dir("lib"));
        assert!(!is_ignored_dir("app"));
        assert!(!is_ignored_dir("services"));
    }

    // ==================== DetectedLanguage Tests ====================

    #[test]
    fn test_detected_language_equality() {
        let lang1 = DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        };

        let lang2 = DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        };

        assert_eq!(lang1, lang2);
    }

    #[test]
    fn test_detected_language_inequality_confidence() {
        let lang1 = DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.7,
            detection_method: DetectionMethod::FileExtension,
        };

        let lang2 = DetectedLanguage {
            name: "javascript".to_string(),
            confidence: 0.95,
            detection_method: DetectionMethod::ConfigFile,
        };

        assert_ne!(lang1, lang2);
    }

    #[test]
    fn test_detection_method_equality() {
        assert_eq!(
            DetectionMethod::FileExtension,
            DetectionMethod::FileExtension
        );
        assert_eq!(DetectionMethod::ConfigFile, DetectionMethod::ConfigFile);
        assert_ne!(DetectionMethod::FileExtension, DetectionMethod::ConfigFile);
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_scan_extensions_deep_nesting() {
        // Files at max depth should be counted
        let temp_dir =
            create_test_repo(&["src/a/b/file1.js", "src/a/b/file2.js", "src/a/b/file3.js"]);

        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
    }

    #[test]
    fn test_scan_extensions_beyond_max_depth() {
        // Files beyond max depth should not be counted
        // (MAX_DEPTH is 3, so src/a/b/c/d would be depth 5 from src)
        let temp_dir = create_test_repo(&[
            "src/a/b/c/d/file1.js",
            "src/a/b/c/d/file2.js",
            "src/a/b/c/d/file3.js",
        ]);

        let detected = scan_file_extensions(temp_dir.path());

        // Files are beyond max depth, so shouldn't be counted
        assert!(detected.is_empty());
    }

    #[test]
    fn test_detect_languages_nonexistent_path() {
        let detected = detect_languages(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(detected.is_empty());
    }

    #[test]
    fn test_scan_extensions_hidden_files() {
        // Hidden files should be counted if they have valid extensions
        let temp_dir = create_test_repo(&[".config.js", ".hidden.js", ".another.js"]);

        let detected = scan_file_extensions(temp_dir.path());

        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "javascript");
    }

    #[test]
    fn test_config_unreadable_package_json() {
        // Test that unreadable files are handled gracefully
        // (This test creates an empty file which should parse but not match patterns)
        let temp_dir = TempDir::new().unwrap();
        create_file_with_content(temp_dir.path(), "package.json", "");

        let detected = check_config_files(temp_dir.path());

        // Empty package.json should still be detected as JavaScript
        // but not TypeScript (no typescript dependency)
        let names: HashSet<_> = detected.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains("javascript"));
        assert!(!names.contains("typescript"));
    }
}
