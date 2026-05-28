use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");

    // Capture git commit hash (short form)
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Get build date: SOURCE_DATE_EPOCH for reproducible builds, otherwise `date`, otherwise unknown
    let build_date = build_date_f();

    // Capture rustc version
    let rustc_version = Command::new("rustc")
        .args(["--version"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| {
                        s.trim()
                            .strip_prefix("rustc ")
                            .unwrap_or(s.trim())
                            .to_string()
                    })
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=NIXMAN_GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=NIXMAN_BUILD_DATE={}", build_date);
    println!("cargo:rustc-env=NIXMAN_RUSTC_VERSION={}", rustc_version);
}

/// Returns today's date as YYYY-MM-DD without pulling in chrono.
/// Falls back to "unknown" when date command is unavailable.
fn build_date_f() -> String {
    // Respect SOURCE_DATE_EPOCH for reproducible builds.
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
        return format!("epoch:{}", epoch);
    }
    // Fallback: try `date`, then "unknown"
    Command::new("date")
        .args(["+%Y-%m-%d"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}
