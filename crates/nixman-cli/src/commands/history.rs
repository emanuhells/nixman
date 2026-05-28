use std::path::Path;


pub async fn run(
    _workspace: &Path,
    show_diff: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let gens = nixman_core::generations::list::all().await?;

    if !show_diff {
        return Ok(serde_json::to_string_pretty(&gens)?);
    }

    let mut results: Vec<serde_json::Value> = Vec::new();
    let gen_list: Vec<&nixman_core::generations::Generation> = gens.iter().collect();

    for (i, gen) in gen_list.iter().enumerate() {
        let mut entry = serde_json::to_value(gen)?;

        if i > 0 {
            let prev = gen_list[i - 1];
            let diff = diff_closures(&prev.path, &gen.path).await;
            entry
                .as_object_mut()
                .unwrap()
                .insert("changes".into(), serde_json::json!(diff));
        }

        results.push(entry);
    }

    Ok(serde_json::to_string_pretty(&results)?)
}

/// Run `nix store diff-closures` between two store paths.
async fn diff_closures(from: &std::path::Path, to: &std::path::Path) -> Vec<String> {
    let output = tokio::process::Command::new("nix")
        .args([
            "store",
            "diff-closures",
            &from.to_string_lossy(),
            &to.to_string_lossy(),
        ])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect(),
        _ => Vec::new(),
    }
}
