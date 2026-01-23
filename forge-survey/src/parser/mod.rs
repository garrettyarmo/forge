//! Parser module for Forge survey.
//!
//! This module provides the framework for language-specific code parsers that
//! extract information from source code to build the knowledge graph.
//!
//! # Architecture
//!
//! The parser architecture is built around the [`Parser`] trait, which all
//! language-specific parsers implement. Each parser is responsible for:
//!
//! 1. Identifying which file extensions it handles
//! 2. Parsing files to extract [`Discovery`] items
//! 3. Walking repository directories (with a sensible default implementation)
//!
//! # Key Principle: Deterministic Execution
//!
//! All parsers in this module use tree-sitter AST parsing only - no LLM calls.
//! This ensures:
//! - **Reproducibility**: Same input code always produces the same discoveries
//! - **Speed**: No API latency or rate limits
//! - **Offline capability**: Works without network access
//!
//! # Available Parsers
//!
//! - [`JavaScriptParser`] - JavaScript/TypeScript (Milestone 2)
//! - [`PythonParser`] - Python (Milestone 3)
//! - [`TerraformParser`] - Terraform/HCL (Milestone 3)
//!
//! # Adding a New Parser
//!
//! To add support for a new language:
//!
//! 1. Create a new file: `parser/{language}.rs`
//! 2. Implement the [`Parser`] trait
//! 3. Add the module to this file and export the parser
//! 4. Register the parser in the parser registry (coming in M3)
//!
//! See the extension guide in `docs/extending-parsers.md` for detailed instructions.

pub mod javascript;
pub mod python;
pub mod terraform;
mod traits;

use std::collections::HashMap;
use std::sync::Arc;

use crate::detection::DetectedLanguages;

// Re-export all public types from traits
pub use traits::{
    ApiCallDiscovery, CloudResourceDiscovery, DatabaseAccessDiscovery, DatabaseOperation,
    Discovery, ImportDiscovery, Parser, ParserError, QueueOperationDiscovery, QueueOperationType,
    ServiceDiscovery,
};

// Re-export parsers
pub use javascript::JavaScriptParser;
pub use python::PythonParser;
pub use terraform::TerraformParser;

/// Registry for language parsers.
///
/// Manages a collection of language-specific parsers that can be retrieved
/// by language name. The registry is thread-safe, using `Arc` to share
/// parser instances across threads.
///
/// # Thread Safety
///
/// All parsers are wrapped in `Arc`, allowing them to be safely shared
/// across multiple threads. The `Parser` trait requires `Send + Sync`,
/// ensuring that parsers can be used in concurrent contexts.
///
/// # Language Mapping
///
/// Some languages share the same parser:
/// - JavaScript and TypeScript both use `JavaScriptParser`
///
/// Language lookup is case-insensitive for convenience.
///
/// # Example
///
/// ```ignore
/// use forge_survey::parser::ParserRegistry;
///
/// let registry = ParserRegistry::new()?;
///
/// // Get a specific parser
/// if let Some(parser) = registry.get("javascript") {
///     let discoveries = parser.parse_repo(&repo_path)?;
/// }
///
/// // Get all parsers for detected languages
/// let detected = detect_languages(&repo_path);
/// let parsers = registry.get_for_languages(&detected, &[]);
/// ```
pub struct ParserRegistry {
    /// Map from lowercase language name to parser instance.
    parsers: HashMap<String, Arc<dyn Parser>>,
}

impl ParserRegistry {
    /// Creates a new parser registry with all built-in parsers registered.
    ///
    /// Automatically registers the following language-to-parser mappings:
    /// - `javascript` -> `JavaScriptParser`
    /// - `typescript` -> `JavaScriptParser` (shared instance)
    /// - `python` -> `PythonParser`
    /// - `terraform` -> `TerraformParser`
    ///
    /// # Errors
    ///
    /// Returns an error if any of the built-in parsers fail to initialize.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let registry = ParserRegistry::new()?;
    /// assert_eq!(registry.available_languages().len(), 4);
    /// ```
    pub fn new() -> Result<Self, ParserError> {
        let mut parsers: HashMap<String, Arc<dyn Parser>> = HashMap::new();

        // Create JavaScript parser (shared between JavaScript and TypeScript)
        let js_parser: Arc<dyn Parser> = Arc::new(JavaScriptParser::new()?);

        // Register JavaScript and TypeScript with the same parser
        parsers.insert("javascript".to_string(), Arc::clone(&js_parser));
        parsers.insert("typescript".to_string(), js_parser);

        // Create and register Python parser
        let python_parser: Arc<dyn Parser> = Arc::new(PythonParser::new()?);
        parsers.insert("python".to_string(), python_parser);

        // Create and register Terraform parser
        let terraform_parser: Arc<dyn Parser> = Arc::new(TerraformParser::new()?);
        parsers.insert("terraform".to_string(), terraform_parser);

        Ok(Self { parsers })
    }

    /// Gets a parser by language name.
    ///
    /// Language lookup is case-insensitive, so "JavaScript", "javascript",
    /// and "JAVASCRIPT" all return the same parser.
    ///
    /// # Arguments
    ///
    /// * `language` - The name of the language to get a parser for.
    ///
    /// # Returns
    ///
    /// Returns `Some(Arc<dyn Parser>)` if a parser exists for the language,
    /// or `None` if no parser is registered for that language.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let registry = ParserRegistry::new()?;
    ///
    /// // Case-insensitive lookup
    /// assert!(registry.get("JavaScript").is_some());
    /// assert!(registry.get("javascript").is_some());
    /// assert!(registry.get("JAVASCRIPT").is_some());
    ///
    /// // Unknown language
    /// assert!(registry.get("cobol").is_none());
    /// ```
    pub fn get(&self, language: &str) -> Option<Arc<dyn Parser>> {
        self.parsers.get(&language.to_lowercase()).cloned()
    }

    /// Gets parsers for all detected languages, excluding specified ones.
    ///
    /// This method filters the detected languages by the exclusion list and
    /// returns unique parser instances. Since JavaScript and TypeScript share
    /// the same parser, if both are detected and neither is excluded, only
    /// one parser instance is returned.
    ///
    /// The exclusion list is also case-insensitive.
    ///
    /// # Arguments
    ///
    /// * `languages` - The detected languages from language detection.
    /// * `exclude` - A list of language names to exclude from the results.
    ///
    /// # Returns
    ///
    /// A vector of unique parser instances for the detected languages
    /// (excluding those in the exclusion list).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let registry = ParserRegistry::new()?;
    /// let detected = detect_languages(&repo_path);
    ///
    /// // Get all parsers
    /// let all_parsers = registry.get_for_languages(&detected, &[]);
    ///
    /// // Exclude Python
    /// let non_python = registry.get_for_languages(&detected, &["python".to_string()]);
    /// ```
    pub fn get_for_languages(
        &self,
        languages: &DetectedLanguages,
        exclude: &[String],
    ) -> Vec<Arc<dyn Parser>> {
        // Convert exclude list to lowercase for case-insensitive comparison
        let exclude_lower: Vec<String> = exclude.iter().map(|s| s.to_lowercase()).collect();

        // Collect parsers while avoiding duplicates
        // Use a separate set to track which Arc pointers we've already added
        let mut result: Vec<Arc<dyn Parser>> = Vec::new();
        let mut seen_ptrs: Vec<*const dyn Parser> = Vec::new();

        for lang in languages.iter() {
            let lang_lower = lang.name.to_lowercase();

            // Skip if this language is in the exclusion list
            if exclude_lower.contains(&lang_lower) {
                continue;
            }

            // Get the parser for this language
            if let Some(parser) = self.parsers.get(&lang_lower) {
                let ptr = Arc::as_ptr(parser);

                // Only add if we haven't seen this parser instance before
                if !seen_ptrs.contains(&ptr) {
                    seen_ptrs.push(ptr);
                    result.push(Arc::clone(parser));
                }
            }
        }

        result
    }

    /// Lists all registered language names.
    ///
    /// Returns the language names in no particular order.
    ///
    /// # Returns
    ///
    /// A vector of language name references.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let registry = ParserRegistry::new()?;
    /// let languages = registry.available_languages();
    ///
    /// assert!(languages.contains(&"javascript"));
    /// assert!(languages.contains(&"typescript"));
    /// assert!(languages.contains(&"python"));
    /// assert!(languages.contains(&"terraform"));
    /// ```
    pub fn available_languages(&self) -> Vec<&str> {
        self.parsers.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::{DetectedLanguage, DetectionMethod};

    // ==================== ParserRegistry::new() Tests ====================

    #[test]
    fn test_registry_new_creates_all_parsers() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        // Should have 4 language mappings
        assert_eq!(registry.parsers.len(), 4);

        // All expected languages should be present
        assert!(registry.parsers.contains_key("javascript"));
        assert!(registry.parsers.contains_key("typescript"));
        assert!(registry.parsers.contains_key("python"));
        assert!(registry.parsers.contains_key("terraform"));
    }

    #[test]
    fn test_registry_javascript_typescript_share_parser() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let js_parser = registry.get("javascript").unwrap();
        let ts_parser = registry.get("typescript").unwrap();

        // They should be the same Arc instance (same pointer)
        assert!(Arc::ptr_eq(&js_parser, &ts_parser));
    }

    #[test]
    fn test_registry_python_separate_parser() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let js_parser = registry.get("javascript").unwrap();
        let py_parser = registry.get("python").unwrap();

        // They should be different Arc instances
        assert!(!Arc::ptr_eq(&js_parser, &py_parser));
    }

    #[test]
    fn test_registry_terraform_separate_parser() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let js_parser = registry.get("javascript").unwrap();
        let tf_parser = registry.get("terraform").unwrap();

        // They should be different Arc instances
        assert!(!Arc::ptr_eq(&js_parser, &tf_parser));
    }

    // ==================== ParserRegistry::get() Tests ====================

    #[test]
    fn test_get_existing_language() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        assert!(registry.get("javascript").is_some());
        assert!(registry.get("typescript").is_some());
        assert!(registry.get("python").is_some());
        assert!(registry.get("terraform").is_some());
    }

    #[test]
    fn test_get_unknown_language_returns_none() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        assert!(registry.get("cobol").is_none());
        assert!(registry.get("fortran").is_none());
        assert!(registry.get("").is_none());
    }

    #[test]
    fn test_get_case_insensitive_lowercase() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let parser1 = registry.get("javascript");
        let parser2 = registry.get("javascript");

        assert!(parser1.is_some());
        assert!(parser2.is_some());
        assert!(Arc::ptr_eq(&parser1.unwrap(), &parser2.unwrap()));
    }

    #[test]
    fn test_get_case_insensitive_uppercase() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let parser1 = registry.get("javascript");
        let parser2 = registry.get("JAVASCRIPT");

        assert!(parser1.is_some());
        assert!(parser2.is_some());
        assert!(Arc::ptr_eq(&parser1.unwrap(), &parser2.unwrap()));
    }

    #[test]
    fn test_get_case_insensitive_mixed_case() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let parser1 = registry.get("javascript");
        let parser2 = registry.get("JavaScript");
        let parser3 = registry.get("jAvAsCrIpT");

        assert!(parser1.is_some());
        assert!(parser2.is_some());
        assert!(parser3.is_some());

        let p1 = parser1.unwrap();
        let p2 = parser2.unwrap();
        let p3 = parser3.unwrap();

        assert!(Arc::ptr_eq(&p1, &p2));
        assert!(Arc::ptr_eq(&p2, &p3));
    }

    #[test]
    fn test_get_case_insensitive_python() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        assert!(registry.get("python").is_some());
        assert!(registry.get("Python").is_some());
        assert!(registry.get("PYTHON").is_some());
    }

    #[test]
    fn test_get_case_insensitive_terraform() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        assert!(registry.get("terraform").is_some());
        assert!(registry.get("Terraform").is_some());
        assert!(registry.get("TERRAFORM").is_some());
    }

    // ==================== ParserRegistry::get_for_languages() Tests ====================

    fn create_detected_languages(names: &[&str]) -> DetectedLanguages {
        let mut detected = DetectedLanguages::new();
        for name in names {
            detected.add(DetectedLanguage {
                name: name.to_string(),
                confidence: 0.95,
                detection_method: DetectionMethod::ConfigFile,
            });
        }
        detected
    }

    #[test]
    fn test_get_for_languages_empty_detected() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = DetectedLanguages::new();

        let parsers = registry.get_for_languages(&detected, &[]);

        assert!(parsers.is_empty());
    }

    #[test]
    fn test_get_for_languages_single_language() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript"]);

        let parsers = registry.get_for_languages(&detected, &[]);

        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_multiple_languages() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "python"]);

        let parsers = registry.get_for_languages(&detected, &[]);

        assert_eq!(parsers.len(), 2);
    }

    #[test]
    fn test_get_for_languages_javascript_typescript_deduplicated() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "typescript"]);

        let parsers = registry.get_for_languages(&detected, &[]);

        // Should only return one parser since they share the same instance
        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_all_four_languages() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected =
            create_detected_languages(&["javascript", "typescript", "python", "terraform"]);

        let parsers = registry.get_for_languages(&detected, &[]);

        // JavaScript and TypeScript share a parser, so only 3 unique parsers
        assert_eq!(parsers.len(), 3);
    }

    #[test]
    fn test_get_for_languages_unknown_language_ignored() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "cobol"]);

        let parsers = registry.get_for_languages(&detected, &[]);

        // Only JavaScript should be returned, COBOL has no parser
        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_exclude_single() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "python"]);

        let parsers = registry.get_for_languages(&detected, &["python".to_string()]);

        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_exclude_multiple() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "python", "terraform"]);

        let parsers =
            registry.get_for_languages(&detected, &["python".to_string(), "terraform".to_string()]);

        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_exclude_all() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "python"]);

        let parsers = registry
            .get_for_languages(&detected, &["javascript".to_string(), "python".to_string()]);

        assert!(parsers.is_empty());
    }

    #[test]
    fn test_get_for_languages_exclude_case_insensitive() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "python"]);

        // Exclude with different case
        let parsers = registry.get_for_languages(&detected, &["PYTHON".to_string()]);

        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_exclude_javascript_keeps_typescript() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "typescript", "python"]);

        // Exclude javascript but not typescript
        let parsers = registry.get_for_languages(&detected, &["javascript".to_string()]);

        // TypeScript parser should still be included (plus Python)
        assert_eq!(parsers.len(), 2);
    }

    #[test]
    fn test_get_for_languages_exclude_both_js_and_ts() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript", "typescript", "python"]);

        // Exclude both javascript and typescript
        let parsers = registry.get_for_languages(
            &detected,
            &["javascript".to_string(), "typescript".to_string()],
        );

        // Only Python should remain
        assert_eq!(parsers.len(), 1);
    }

    #[test]
    fn test_get_for_languages_exclude_nonexistent() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let detected = create_detected_languages(&["javascript"]);

        // Exclude a language that doesn't exist in detected
        let parsers = registry.get_for_languages(&detected, &["cobol".to_string()]);

        // Should still return javascript parser
        assert_eq!(parsers.len(), 1);
    }

    // ==================== ParserRegistry::available_languages() Tests ====================

    #[test]
    fn test_available_languages_returns_all() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let languages = registry.available_languages();

        assert_eq!(languages.len(), 4);
        assert!(languages.contains(&"javascript"));
        assert!(languages.contains(&"typescript"));
        assert!(languages.contains(&"python"));
        assert!(languages.contains(&"terraform"));
    }

    #[test]
    fn test_available_languages_lowercase() {
        let registry = ParserRegistry::new().expect("Failed to create registry");

        let languages = registry.available_languages();

        // All language names should be lowercase
        for lang in languages {
            assert_eq!(lang, lang.to_lowercase());
        }
    }

    // ==================== Parser Functionality Tests ====================

    #[test]
    fn test_retrieved_javascript_parser_works() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("javascript").unwrap();

        let content = r#"import express from 'express';"#;
        let discoveries = parser
            .parse_file(std::path::Path::new("test.js"), content)
            .unwrap();

        assert!(!discoveries.is_empty());
    }

    #[test]
    fn test_retrieved_python_parser_works() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("python").unwrap();

        let content = r#"import boto3"#;
        let discoveries = parser
            .parse_file(std::path::Path::new("test.py"), content)
            .unwrap();

        assert!(!discoveries.is_empty());
    }

    #[test]
    fn test_retrieved_terraform_parser_works() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("terraform").unwrap();

        let content = r#"
resource "aws_dynamodb_table" "test" {
  name = "test-table"
}
"#;
        let discoveries = parser
            .parse_file(std::path::Path::new("main.tf"), content)
            .unwrap();

        assert!(!discoveries.is_empty());
    }

    #[test]
    fn test_supported_extensions_javascript() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("javascript").unwrap();

        let extensions = parser.supported_extensions();

        assert!(extensions.contains(&"js"));
        assert!(extensions.contains(&"jsx"));
        assert!(extensions.contains(&"ts"));
        assert!(extensions.contains(&"tsx"));
    }

    #[test]
    fn test_supported_extensions_python() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("python").unwrap();

        let extensions = parser.supported_extensions();

        assert!(extensions.contains(&"py"));
    }

    #[test]
    fn test_supported_extensions_terraform() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("terraform").unwrap();

        let extensions = parser.supported_extensions();

        assert!(extensions.contains(&"tf"));
    }

    // ==================== Thread Safety Tests ====================

    #[test]
    fn test_parser_can_be_cloned_and_shared() {
        let registry = ParserRegistry::new().expect("Failed to create registry");
        let parser = registry.get("javascript").unwrap();

        // Clone the Arc
        let parser_clone = Arc::clone(&parser);

        // Both should work
        let content = r#"import x from 'y';"#;
        let d1 = parser
            .parse_file(std::path::Path::new("test.js"), content)
            .unwrap();
        let d2 = parser_clone
            .parse_file(std::path::Path::new("test.js"), content)
            .unwrap();

        assert_eq!(d1.len(), d2.len());
    }

    #[test]
    fn test_multiple_registries_independent() {
        let registry1 = ParserRegistry::new().expect("Failed to create registry");
        let registry2 = ParserRegistry::new().expect("Failed to create registry");

        let parser1 = registry1.get("javascript").unwrap();
        let parser2 = registry2.get("javascript").unwrap();

        // Different registries should have different parser instances
        assert!(!Arc::ptr_eq(&parser1, &parser2));
    }
}
