use std::path::Path;

#[derive(serde::Serialize)]
struct CheckResult {
    name: String,
    passed: bool,
    message: String,
}

pub async fn run(
    _workspace: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut checks: Vec<CheckResult> = Vec::new();

    // 1. Network connectivity
    checks.push(check_network().await);

    // 2. DNS resolution
    checks.push(check_dns().await);

    // 3. Display manager
    checks.push(check_display_manager().await);

    // 4. Audio
    checks.push(check_audio().await);

    // 5. Failed services
    checks.push(check_failed_services().await);

    // 6. Filesystems
    checks.push(check_filesystems().await);

    let all_passed = checks.iter().all(|c| c.passed);
    let result = serde_json::json!({
        "healthy": all_passed,
        "checks": checks,
    });

    Ok(serde_json::to_string_pretty(&result)?)
}

async fn check_network() -> CheckResult {
    let output = tokio::process::Command::new("ip")
        .args(["route", "show", "default"])
        .output().await;

    let gateway = match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .nth(2)  // "default via X.X.X.X ..."
                .map(|s| s.to_string())
        }
        _ => None,
    };

    if let Some(gw) = gateway {
        let ping = tokio::process::Command::new("ping")
            .args(["-c", "1", "-W", "3", &gw])
            .output().await;
        match ping {
            Ok(o) if o.status.success() => CheckResult {
                name: "network".into(),
                passed: true,
                message: format!("Gateway {} reachable", gw),
            },
            _ => CheckResult {
                name: "network".into(),
                passed: false,
                message: format!("Cannot reach gateway {}", gw),
            },
        }
    } else {
        CheckResult {
            name: "network".into(),
            passed: false,
            message: "No default route found".into(),
        }
    }
}

async fn check_dns() -> CheckResult {
    let output = tokio::process::Command::new("getent").args(["hosts", "nixos.org"])
        .output().await;
    match output {
        Ok(o) if o.status.success() => CheckResult {
            name: "dns".into(),
            passed: true,
            message: "DNS resolution working".into(),
        },
        _ => CheckResult {
            name: "dns".into(),
            passed: false,
            message: "DNS resolution failed (getent hosts nixos.org)".into(),
        },
    }
}

async fn check_display_manager() -> CheckResult {
    for dm in &["sddm", "gdm", "lightdm", "greetd"] {
        let unit = format!("{}.service", dm);
        let output = tokio::process::Command::new("systemctl")
            .args(["is-active", &unit])
            .output().await;
        if let Ok(o) = output {
            if o.status.success() {
                return CheckResult {
                    name: "display_manager".into(),
                    passed: true,
                    message: format!("{} active", unit),
                };
            }
        }
    }
    CheckResult {
        name: "display_manager".into(),
        passed: true,
        message: "No display manager detected (headless?)".into(),
    }
}

async fn check_audio() -> CheckResult {
    for svc in &["pipewire.service", "pulseaudio.service"] {
        let output = tokio::process::Command::new("systemctl")
            .args(["--user", "is-active", svc])
            .output().await;
        if let Ok(o) = output {
            if o.status.success() {
                return CheckResult {
                    name: "audio".into(),
                    passed: true,
                    message: format!("{} active", svc),
                };
            }
        }
    }
    CheckResult {
        name: "audio".into(),
        passed: true,
        message: "No audio service detected (headless?)".into(),
    }
}

async fn check_failed_services() -> CheckResult {
    let output = tokio::process::Command::new("systemctl")
        .args(["--failed", "--no-legend", "--plain"])
        .output().await;
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let failed: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
            if failed.is_empty() {
                CheckResult {
                    name: "services".into(),
                    passed: true,
                    message: "No failed services".into(),
                }
            } else {
                CheckResult {
                    name: "services".into(),
                    passed: false,
                    message: format!("{} failed service(s): {}", failed.len(),
                        failed.iter().take(5).map(|l| l.split_whitespace().next().unwrap_or("")).collect::<Vec<_>>().join(", ")),
                }
            }
        }
        _ => CheckResult {
            name: "services".into(),
            passed: true,
            message: "Could not check service status".into(),
        },
    }
}

async fn check_filesystems() -> CheckResult {
    let output = tokio::process::Command::new("df")
        .args(["-h", "/", "/nix"])
        .output().await;
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let mut warnings = Vec::new();
            for line in text.lines().skip(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 {
                    if let Some(pct_str) = parts[4].strip_suffix('%') {
                        if let Ok(pct) = pct_str.parse::<u32>() {
                            if pct > 90 {
                                warnings.push(format!("{} at {}%", parts[5], pct));
                            }
                        }
                    }
                }
            }
            if warnings.is_empty() {
                CheckResult {
                    name: "filesystems".into(),
                    passed: true,
                    message: "Disk usage normal".into(),
                }
            } else {
                CheckResult {
                    name: "filesystems".into(),
                    passed: false,
                    message: format!("High disk usage: {}", warnings.join(", ")),
                }
            }
        }
        _ => CheckResult {
            name: "filesystems".into(),
            passed: true,
            message: "Could not check disk usage".into(),
        },
    }
}
