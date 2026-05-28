//! Garbage collection for the Nix store.
//!
//! Optionally deletes old generations first, then runs `nix-collect-garbage`
//! and reports how many bytes were freed.
//!
//! The `keep_last` parameter maps directly to the `+N` syntax accepted by
//! `nix-env --delete-generations`: generations older than the last N are
//! removed before the GC sweep.

use tokio::process::Command;

use crate::generations::types::{GcResult, GenerationError};

/// Profile path managed by NixOS.
const SYSTEM_PROFILE: &str = "/nix/var/nix/profiles/system";

/// Run Nix garbage collection, optionally pruning old generations first.
///
/// If `keep_last` is `Some(n)`, generations older than the last `n` are
/// deleted with `nix-env --delete-generations +N` before GC runs.
///
/// Returns a [`GcResult`] containing the freed byte count (parsed from
/// `nix-collect-garbage` output) and the list of deleted generation numbers.
///
/// # Errors
/// - [`GenerationError::CommandFailed`] if either command exits non-zero.
/// - [`GenerationError::IoError`] if a command cannot be spawned.
pub async fn collect(keep_last: Option<u32>) -> Result<GcResult, GenerationError> {
    let mut deleted_generations: Vec<u32> = Vec::new();

    // ── optional: prune old generations ──────────────────────────────────────
    if let Some(keep) = keep_last {
        let gen_spec = format!("+{keep}");

        let del_out = Command::new("nix-env")
            .args(["--profile", SYSTEM_PROFILE, "--delete-generations", &gen_spec])
            .output()
            .await
            .map_err(GenerationError::IoError)?;

        // Parse "removing generation N" lines from stdout.
        let stdout = String::from_utf8_lossy(&del_out.stdout);
        for line in stdout.lines() {
            if let Some(num) = parse_deleted_generation(line) {
                deleted_generations.push(num);
            }
        }

        if !del_out.status.success() {
            return Err(GenerationError::CommandFailed {
                exit_code: del_out.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&del_out.stderr).into_owned(),
            });
        }
    }

    // ── garbage collection ────────────────────────────────────────────────────
    let gc_out = Command::new("nix-collect-garbage")
        .output()
        .await
        .map_err(GenerationError::IoError)?;

    if !gc_out.status.success() {
        return Err(GenerationError::CommandFailed {
            exit_code: gc_out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&gc_out.stderr).into_owned(),
        });
    }

    let gc_stdout = String::from_utf8_lossy(&gc_out.stdout);
    let freed_bytes = parse_freed_bytes(&gc_stdout);

    deleted_generations.sort();

    Ok(GcResult {
        freed_bytes,
        deleted_generations,
    })
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Parse a line like `"removing generation 5"` and return `Some(5)`.
fn parse_deleted_generation(line: &str) -> Option<u32> {
    line.trim()
        .strip_prefix("removing generation ")
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// Parse freed bytes from `nix-collect-garbage` output.
///
/// Looks for a line in the form:
/// ```text
/// N store paths deleted, X.YZ MiB freed.
/// ```
/// Supports units: `B`, `KiB`, `MiB`, `GiB`, `TiB`.
/// Returns `0` if the line cannot be found or parsed.
fn parse_freed_bytes(output: &str) -> u64 {
    for line in output.lines() {
        if !line.contains("freed") {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();

        // Find the "freed" or "freed." token and look back two positions for
        // <value> <unit>.
        for (i, token) in parts.iter().enumerate() {
            if token.trim_end_matches('.') == "freed" && i >= 2 {
                let unit = parts[i - 1].trim_end_matches('.');
                if let Ok(value) = parts[i - 2].parse::<f64>() {
                    let multiplier: u64 = match unit {
                        "B" => 1,
                        "KiB" => 1_024,
                        "MiB" => 1_024 * 1_024,
                        "GiB" => 1_024 * 1_024 * 1_024,
                        "TiB" => 1_024 * 1_024 * 1_024 * 1_024,
                        _ => 1,
                    };
                    return (value * multiplier as f64) as u64;
                }
            }
        }
    }
    0
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_deleted_generation_valid() {
        assert_eq!(parse_deleted_generation("removing generation 5"), Some(5));
        assert_eq!(parse_deleted_generation("  removing generation 12  "), Some(12));
    }

    #[test]
    fn parse_deleted_generation_invalid() {
        assert_eq!(parse_deleted_generation("keeping generation 3"), None);
        assert_eq!(parse_deleted_generation("removing generation abc"), None);
        assert_eq!(parse_deleted_generation(""), None);
    }

    #[test]
    fn parse_freed_bytes_mib() {
        let output = "23 store paths deleted, 1.23 MiB freed.\n";
        let bytes = parse_freed_bytes(output);
        // 1.23 * 1024 * 1024 = 1289748.48 → truncated to 1289748
        assert_eq!(bytes, (1.23_f64 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn parse_freed_bytes_gib() {
        let output = "100 store paths deleted, 2.50 GiB freed.\n";
        let bytes = parse_freed_bytes(output);
        assert_eq!(bytes, (2.50_f64 * 1024.0 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn parse_freed_bytes_zero_paths() {
        // No paths deleted → "0.00 MiB freed."
        let output = "0 store paths deleted, 0.00 MiB freed.\n";
        assert_eq!(parse_freed_bytes(output), 0);
    }

    #[test]
    fn parse_freed_bytes_no_match() {
        assert_eq!(parse_freed_bytes("nothing happened\n"), 0);
    }

    #[test]
    fn gc_signature_exists() {
        let _: fn(Option<u32>) -> _ = super::collect;
    }
}
