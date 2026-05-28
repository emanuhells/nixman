use clap::CommandFactory;
use clap_complete::{generate, Shell};


pub async fn run(shell: Shell) -> Result<String, Box<dyn std::error::Error>> {
    let install_hint = match shell {
        Shell::Bash => "# Add to ~/.bashrc:\neval \"$(nixman completions bash)\"",
        Shell::Zsh => "# Add to ~/.zshrc:\neval \"$(nixman completions zsh)\"",
        Shell::Fish => "# Run once:\nnixman completions fish | source",
        _ => "# See shell documentation for completion installation",
    };
    eprintln!("{}\n", install_hint);

    let mut cmd = crate::Cli::command();
    let mut buf = Vec::new();
    generate(shell, &mut cmd, "nixman", &mut buf);
    Ok(String::from_utf8(buf)?)
}

/// Hidden command for dynamic option path completion.
/// Called by shell completion scripts to suggest option paths.
pub async fn complete_option(prefix: &str, _workspace: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let cache_dir = nixman_core::options::cache::default_cache_dir();

    let index = find_cached_index(&cache_dir);

    let Some(index) = index else {
        return Ok(String::new());
    };

    let prefix_with_dot = if prefix.is_empty() || prefix.ends_with('.') {
        prefix.to_string()
    } else {
        // Partial segment completion — treat as starts-with
        prefix.to_string()
    };

    let matches: Vec<&str> = index.options.iter()
        .filter(|opt| opt.path.starts_with(&prefix_with_dot))
        .map(|opt| {
            // Return the next segment only
            let rest = &opt.path[prefix_with_dot.len()..];
            if let Some(dot_pos) = rest.find('.') {
                // There's more segments — return up to next dot
                &opt.path[..prefix_with_dot.len() + dot_pos + 1]
            } else {
                // This is a leaf option
                opt.path.as_str()
            }
        })
        .collect();

    // Deduplicate (many options share prefixes)
    let mut unique: Vec<&str> = matches.clone();
    unique.sort();
    unique.dedup();

    let result = unique.into_iter().take(50).collect::<Vec<_>>().join("\n");
    Ok(result)
}

/// Find any cached option index in the cache directory.
fn find_cached_index(cache_dir: &std::path::Path) -> Option<nixman_core::options::OptionIndex> {
    let entries = std::fs::read_dir(cache_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("options-") && name.ends_with(".json") {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(index) = serde_json::from_str::<nixman_core::options::OptionIndex>(&content) {
                    return Some(index);
                }
            }
        }
    }
    None
}
