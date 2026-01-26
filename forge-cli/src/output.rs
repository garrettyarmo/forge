use console::{Emoji, style};

pub static SUCCESS: Emoji<'_, '_> = Emoji("✅ ", "OK ");
pub static WARNING: Emoji<'_, '_> = Emoji("⚠️  ", "!! ");
pub static ERROR: Emoji<'_, '_> = Emoji("❌ ", "ERR ");
pub static INFO: Emoji<'_, '_> = Emoji("ℹ️  ", "i ");

/// Print a success message
pub fn success(msg: &str) {
    println!("{} {}", SUCCESS, style(msg).green());
}

/// Print a warning message
pub fn warning(msg: &str) {
    eprintln!("{} {}", WARNING, style(msg).yellow());
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("{} {}", ERROR, style(msg).red().bold());
}

/// Print an info message
pub fn info(msg: &str) {
    println!("{} {}", INFO, style(msg).cyan());
}

/// Print a heading
#[allow(dead_code)]
pub fn heading(msg: &str) {
    println!("\n{}\n{}", style(msg).bold(), "=".repeat(msg.len()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_functions() {
        // Smoke test - just ensure they don't panic
        success("Test success");
        warning("Test warning");
        error("Test error");
        info("Test info");
        heading("Test Heading");
    }

    #[test]
    fn test_emoji_constants() {
        // Ensure emojis are defined
        assert!(!SUCCESS.to_string().is_empty());
        assert!(!WARNING.to_string().is_empty());
        assert!(!ERROR.to_string().is_empty());
        assert!(!INFO.to_string().is_empty());
    }
}
