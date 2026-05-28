//! Interactive confirmation prompts.

use std::io::{self, IsTerminal, Write};

/// Ask user for confirmation. Returns Ok(true) if confirmed.
/// - If `--yes` was passed: always returns Ok(true) without prompting.
/// - If stdin is not a TTY and --yes not passed: returns Err (would hang).
#[allow(dead_code)]
pub fn confirm(prompt: &str, yes_flag: bool) -> Result<bool, String> {
    if yes_flag {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Err("requires --yes flag in non-interactive mode".to_string());
    }
    eprint!("{} [Y/n] ", prompt);
    io::stderr().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;
    let trimmed = input.trim().to_lowercase();
    Ok(trimmed.is_empty() || trimmed == "y" || trimmed == "yes")
}
