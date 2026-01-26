use console::{Emoji, style};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Global quiet mode flag - when true, suppresses all output except errors.
static QUIET_MODE: AtomicBool = AtomicBool::new(false);

/// Global verbosity level (0 = normal, 1+ = verbose).
static VERBOSITY: AtomicU8 = AtomicU8::new(0);

pub static SUCCESS: Emoji<'_, '_> = Emoji("✅ ", "OK ");
pub static WARNING: Emoji<'_, '_> = Emoji("⚠️  ", "!! ");
pub static ERROR: Emoji<'_, '_> = Emoji("❌ ", "ERR ");
pub static INFO: Emoji<'_, '_> = Emoji("ℹ️  ", "i ");

/// Set quiet mode globally.
///
/// When quiet mode is enabled, all output functions except `error()` will be suppressed.
pub fn set_quiet(quiet: bool) {
    QUIET_MODE.store(quiet, Ordering::SeqCst);
}

/// Check if quiet mode is enabled.
pub fn is_quiet() -> bool {
    QUIET_MODE.load(Ordering::SeqCst)
}

/// Set verbosity level globally.
///
/// Level 0 is normal output, level 1+ enables verbose output.
pub fn set_verbosity(level: u8) {
    VERBOSITY.store(level, Ordering::SeqCst);
}

/// Get the current verbosity level.
#[allow(dead_code)]
pub fn get_verbosity() -> u8 {
    VERBOSITY.load(Ordering::SeqCst)
}

/// Check if verbose mode is enabled (verbosity level >= 1).
pub fn is_verbose() -> bool {
    VERBOSITY.load(Ordering::SeqCst) >= 1
}

/// Print a success message (suppressed in quiet mode).
pub fn success(msg: &str) {
    if !is_quiet() {
        println!("{} {}", SUCCESS, style(msg).green());
    }
}

/// Print a warning message (suppressed in quiet mode).
pub fn warning(msg: &str) {
    if !is_quiet() {
        eprintln!("{} {}", WARNING, style(msg).yellow());
    }
}

/// Print an error message (NEVER suppressed, even in quiet mode).
pub fn error(msg: &str) {
    eprintln!("{} {}", ERROR, style(msg).red().bold());
}

/// Print an info message (suppressed in quiet mode).
pub fn info(msg: &str) {
    if !is_quiet() {
        println!("{} {}", INFO, style(msg).cyan());
    }
}

/// Print a verbose message (only shown when verbosity >= 1 and not in quiet mode).
pub fn verbose(msg: &str) {
    if is_verbose() && !is_quiet() {
        println!("{}", style(msg).dim());
    }
}

/// Print a heading (suppressed in quiet mode).
#[allow(dead_code)]
pub fn heading(msg: &str) {
    if !is_quiet() {
        println!("\n{}\n{}", style(msg).bold(), "=".repeat(msg.len()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_functions() {
        // Reset state for test
        set_quiet(false);
        set_verbosity(0);

        // Smoke test - just ensure they don't panic
        success("Test success");
        warning("Test warning");
        error("Test error");
        info("Test info");
        heading("Test Heading");
        verbose("Test verbose"); // Won't print since verbosity is 0
    }

    #[test]
    fn test_emoji_constants() {
        // Ensure emojis are defined
        assert!(!SUCCESS.to_string().is_empty());
        assert!(!WARNING.to_string().is_empty());
        assert!(!ERROR.to_string().is_empty());
        assert!(!INFO.to_string().is_empty());
    }

    #[test]
    fn test_quiet_mode() {
        // Test quiet mode accessors
        set_quiet(false);
        assert!(!is_quiet());

        set_quiet(true);
        assert!(is_quiet());

        // Reset for other tests
        set_quiet(false);
    }

    #[test]
    fn test_verbosity() {
        // Test verbosity accessors
        set_verbosity(0);
        assert_eq!(get_verbosity(), 0);
        assert!(!is_verbose());

        set_verbosity(1);
        assert_eq!(get_verbosity(), 1);
        assert!(is_verbose());

        set_verbosity(2);
        assert_eq!(get_verbosity(), 2);
        assert!(is_verbose());

        // Reset for other tests
        set_verbosity(0);
    }

    #[test]
    fn test_verbose_output() {
        // Reset state
        set_quiet(false);
        set_verbosity(0);

        // Verbose should not panic even when not in verbose mode
        verbose("This won't print");

        // Enable verbose mode
        set_verbosity(1);
        verbose("This will print");

        // Reset
        set_verbosity(0);
    }
}
