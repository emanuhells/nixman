use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;

/// Compute the same hash that pending_store uses for workspace paths.
fn workspace_hash(workspace: &Path) -> String {
    let ws_str = workspace.to_string_lossy();
    let hash: u64 = ws_str
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    format!("{:016x}", hash)
}

// ── Existing tests ───────────────────────────────────────────────────

#[test]
fn test_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage NixOS configuration"));
}

#[test]
fn test_version() {
    Command::cargo_bin("nixman")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("0.1.0"));
}

#[test]
fn test_workspace_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["workspace", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("detect"));
}

#[test]
fn test_option_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["option", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("search"));
}

#[test]
fn test_packages_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["packages", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("search"));
}

#[test]
fn test_generations_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["generations", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"));
}

#[test]
fn test_rebuild_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["rebuild", "--help"])
        .assert()
        .success();
}

#[test]
fn test_invalid_command() {
    Command::cargo_bin("nixman")
        .unwrap()
        .arg("nonexistent")
        .assert()
        .failure();
}

#[test]
fn option_remove_rejects_missing_workspace() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp/nonexistent-nixman-test", "option", "remove", "services.nginx.enable"])
        .assert()
        .failure()
        .stderr(
            predicates::str::contains("resolve error")
                .or(predicates::str::contains("No such file"))
                .or(predicates::str::contains("nix is not installed"))
        );
}

// ── Section 1: HM command structure ──────────────────────────────────

#[test]
fn hm_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["hm", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("option"))
        .stdout(predicate::str::contains("packages"))
        .stdout(predicate::str::contains("rebuild"));
}

#[test]
fn hm_status_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["hm", "status", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Show Home Manager workspace status"));
}

#[test]
fn hm_option_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["hm", "option", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("search"));
}

#[test]
fn hm_packages_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["hm", "packages", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("remove"));
}

#[test]
fn hm_rebuild_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["hm", "rebuild", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("switch"))
        .stdout(predicate::str::contains("build"))
        .stdout(predicate::str::contains("boot"))
        .stdout(predicate::str::contains("test"));
}

// ── Section 2: HM error handling ─────────────────────────────────────

#[test]
fn hm_status_fails_without_workspace() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["hm", "status"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Home Manager workspace not found"));
}

// ── Section 3: Pending store integration ─────────────────────────────

#[test]
fn pending_list_shows_manual_file() {
    let ws = std::env::temp_dir().join("nixman-test-manual-pending");
    let state = std::env::temp_dir().join("nixman-test-state-manual");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    let hash = workspace_hash(&ws);
    let pending_dir = state.join("nixman");
    std::fs::create_dir_all(&pending_dir).unwrap();
    let pending_json = r#"{"changes":[{"kind":"option_set","option_path":"services.test.enable","value":"true","timestamp":"1700000000"}]}"#;
    std::fs::write(
        pending_dir.join(format!("pending-{}.json", hash)),
        pending_json,
    )
    .unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "pending", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("services.test.enable"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}

#[test]
fn pending_list_shows_staged_option() {
    let ws = std::env::temp_dir().join("nixman-test-staged-pending");
    let state = std::env::temp_dir().join("nixman-test-state-staged");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    // Stage an option change via --stage flag
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args([
            "--workspace",
            &ws.to_string_lossy(),
            "option",
            "set",
            "services.test.enable",
            "true",
            "--stage",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Staged: services.test.enable = true"));

    // List pending changes — should show the staged entry
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "pending", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("services.test.enable"))
        .stdout(predicate::str::contains("true"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}

// ── Section 4: Existing command sanity checks ────────────────────────

#[test]
fn packages_list_returns_output() {
    let ws = std::env::temp_dir().join("nixman-test-pkgs-list");
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        ws.join("configuration.nix"),
        "{ ... }: { environment.systemPackages = [ ]; }",
    )
    .unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", &ws.to_string_lossy(), "packages", "list"])
        .assert()
        .success();

    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn packages_search_nginx_returns_json() {
    // so `nix search <path>#nixpkgs nginx --json` can resolve packages.
    let ws = std::env::temp_dir().join("nixman-test-pkgs-search");
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        ws.join("flake.nix"),
        r#"{
  description = "nixman test workspace";
  inputs.nixpkgs.url = "nixpkgs";
  outputs = { nixpkgs, ... }: {
    nixpkgs = nixpkgs;
  };
}"#,
    )
    .unwrap();

    // Lock the flake so nix search can evaluate without network fetch
    let lock = std::process::Command::new("nix")
        .args(["flake", "lock", &ws.to_string_lossy()])
        .output();

    match lock {
        Ok(ref out) if out.status.success() => {}
        _ => {
            let _ = std::fs::remove_dir_all(&ws);
            return; // skip if flake locking fails (no nix or no network)
        }
    }

    Command::cargo_bin("nixman")
        .unwrap()
        .args([
            "--workspace",
            &ws.to_string_lossy(),
            "packages",
            "search",
            "nginx",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("nginx"));

    let _ = std::fs::remove_dir_all(&ws);
}

// ── Section 5: Schema and explain commands ───────────────────────────

#[test]
fn schema_outputs_valid_json() {
    let output = Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "schema"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["name"], "nixman");
    assert!(parsed["commands"]["try"].is_object(), "schema should include 'try' command");
    assert!(parsed["commands"]["hm"].is_object(), "schema should include 'hm' command");
    assert!(parsed["commands"]["explain"].is_object(), "schema should include 'explain' command");
    assert!(parsed["commands"]["migrate"].is_object(), "schema should include 'migrate' command");
    assert!(parsed["commands"]["history"].is_object(), "schema should include 'history' command");
}

#[test]
fn explain_known_error_returns_fix() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "explain", "error: The option 'services.foo.enable' does not exist"])
        .assert()
        .success()
        .stdout(predicate::str::contains("option_removed_or_renamed"))
        .stdout(predicate::str::contains("fix"));
}

#[test]
fn explain_assertion_failure() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "explain", "Failed assertion: services.xserver conflicts with services.wayland"])
        .assert()
        .success()
        .stdout(predicate::str::contains("assertion_failure"));
}

#[test]
fn explain_unknown_error() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "explain", "xyzzy frobnicate the widget"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"understood\": false"));
}

#[test]
fn explain_stdin_flag_without_input() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "explain", "--stdin"])
        .write_stdin("error: undefined variable 'pkgs'\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("undefined_variable"));
}

#[test]
fn explain_no_args_returns_error() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "explain"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Provide error text"));
}

// ── Section 6: Try command structure ─────────────────────────────────

#[test]
fn try_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["try", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("apply"))
        .stdout(predicate::str::contains("confirm"));
}

#[test]
fn try_apply_no_changes_errors() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "try", "apply"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No changes specified"));
}

#[test]
fn try_confirm_no_session_errors() {
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", "/tmp/nixman-test-try-nostate")
        .args(["--workspace", "/tmp", "try", "confirm"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No active try session"));
}

// ── Section 7: Intent command structure ──────────────────────────────

#[test]
fn intent_help() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["intent", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("propose"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("apply"))
        .stdout(predicate::str::contains("discard"));
}

#[test]
fn intent_propose_no_changes_errors() {
    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", "/tmp", "intent", "propose"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No changes specified"));
}

#[test]
fn intent_show_no_plan_errors() {
    let ws = std::env::temp_dir().join("nixman-test-intent-noplan");
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", &ws.to_string_lossy(), "intent", "show"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No pending plan"));

    let _ = std::fs::remove_dir_all(&ws);
}

// ── Section 8: Migrate command ───────────────────────────────────────

#[test]
fn migrate_clean_workspace() {
    let ws = std::env::temp_dir().join("nixman-test-migrate");
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        ws.join("configuration.nix"),
        "{ ... }: { services.openssh.enable = true; }",
    )
    .unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", &ws.to_string_lossy(), "migrate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("up_to_date"));

    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn migrate_detects_deprecated_option() {
    let ws = std::env::temp_dir().join("nixman-test-migrate-deprecated");
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        ws.join("configuration.nix"),
        "{ ... }: { services.xserver.displayManager.sddm.enable = true; }",
    )
    .unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .args(["--workspace", &ws.to_string_lossy(), "migrate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("issues_found"))
        .stdout(predicate::str::contains("services.xserver.displayManager.sddm"));

    let _ = std::fs::remove_dir_all(&ws);
}

// ── Section 9: Check and diff commands ───────────────────────────────

#[test]
fn diff_staged_empty() {
    let ws = std::env::temp_dir().join("nixman-test-diff-staged");
    let state = std::env::temp_dir().join("nixman-test-diff-state");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "diff", "--staged"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No staged changes"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}

// ── Section 10: Pending discard ──────────────────────────────────────

#[test]
fn pending_discard_when_empty() {
    let ws = std::env::temp_dir().join("nixman-test-discard-empty");
    let state = std::env::temp_dir().join("nixman-test-discard-state");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "pending", "discard"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No pending changes to discard"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}

#[test]
fn pending_stage_then_discard() {
    let ws = std::env::temp_dir().join("nixman-test-stage-discard");
    let state = std::env::temp_dir().join("nixman-test-sd-state");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    // Stage a change
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args([
            "--workspace", &ws.to_string_lossy(),
            "option", "set", "test.enable", "true", "--stage",
        ])
        .assert()
        .success();

    // Discard
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "pending", "discard"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Discarded 1 staged change"));

    // Confirm empty
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "pending", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No pending changes"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}

// ── Section 11: Package staging ──────────────────────────────────────

#[test]
fn packages_add_stage() {
    let ws = std::env::temp_dir().join("nixman-test-pkg-stage");
    let state = std::env::temp_dir().join("nixman-test-pkg-stage-state");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args([
            "--workspace", &ws.to_string_lossy(),
            "packages", "add", "htop", "--stage", "--no-verify",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Staged: add package 'htop'"));

    // Verify it shows in pending list
    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args(["--workspace", &ws.to_string_lossy(), "pending", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("htop"))
        .stdout(predicate::str::contains("package_add"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}

#[test]
fn packages_remove_stage() {
    let ws = std::env::temp_dir().join("nixman-test-pkg-rm-stage");
    let state = std::env::temp_dir().join("nixman-test-pkg-rm-state");
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
    std::fs::create_dir_all(&ws).unwrap();

    Command::cargo_bin("nixman")
        .unwrap()
        .env("XDG_STATE_HOME", &state)
        .args([
            "--workspace", &ws.to_string_lossy(),
            "packages", "remove", "firefox", "--stage",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Staged: remove package 'firefox'"));

    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&state);
}
