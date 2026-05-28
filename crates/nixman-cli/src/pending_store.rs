//! File-backed staging of pending changes.
//!
//! Changes are persisted to `$XDG_STATE_HOME/nixman/pending-<hash>.json`
//! so they survive between CLI invocations.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Default kind for backward compat with existing staged files.
fn default_kind() -> String {
    "option_set".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedChange {
    /// Kind of change: "option_set", "package_add", or "package_remove".
    #[serde(default = "default_kind")]
    pub kind: String,
    /// For option_set: the Nix option path.
    /// For package_add/remove: the package name.
    pub option_path: String,
    /// For option_set: the raw Nix value string.
    /// For package_add/remove: unused (empty string).
    pub value: String,
    /// Target file override (for package ops; None = auto-detect).
    #[serde(default)]
    pub file: Option<String>,
    pub timestamp: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StagedChanges {
    pub changes: Vec<StagedChange>,
}

fn now_timestamp() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}

impl StagedChanges {
    /// Load from the staging file for this workspace. Returns empty if file doesn't exist.
    pub fn load(workspace: &Path) -> Self {
        let path = staging_path(workspace);
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save to the staging file.
    pub fn save(&self, workspace: &Path) -> Result<(), String> {
        let path = staging_path(workspace);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())
    }

    /// Add or replace a change with the given properties.
    pub fn add(
        &mut self,
        option_path: String,
        value: String,
        kind: String,
        file: Option<String>,
    ) {
        if let Some(existing) = self
            .changes
            .iter_mut()
            .find(|c| c.kind == kind && c.option_path == option_path)
        {
            existing.value = value;
            existing.file = file;
            existing.timestamp = now_timestamp();
        } else {
            self.changes.push(StagedChange {
                kind,
                option_path,
                value,
                file,
                timestamp: now_timestamp(),
            });
        }
    }

    /// Convenience: stage an option set (backward compat helper).
    pub fn add_option(&mut self, option_path: String, value: String) {
        self.add(option_path, value, "option_set".into(), None);
    }

    /// Convenience: stage a package addition.
    pub fn add_package_add(&mut self, package_name: String, file: Option<String>) {
        self.add(package_name.clone(), String::new(), "package_add".into(), file);
    }

    /// Convenience: stage a package removal.
    pub fn add_package_remove(&mut self, package_name: String, file: Option<String>) {
        self.add(package_name.clone(), String::new(), "package_remove".into(), file);
    }

    /// Remove the staging file.
    pub fn discard(workspace: &Path) {
        let path = staging_path(workspace);
        let _ = std::fs::remove_file(path);
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn count(&self) -> usize {
        self.changes.len()
    }
}

/// Compute the staging file path: $XDG_STATE_HOME/nixman/pending-<hash>.json
/// Hash is a 64-bit polynomial hash of the workspace path (for uniqueness).
fn staging_path(workspace: &Path) -> PathBuf {
    let state_dir = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".local").join("state")
        });

    let workspace_str = workspace.to_string_lossy();
    let hash: u64 = workspace_str
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    let filename = format!("pending-{:016x}.json", hash);

    state_dir.join("nixman").join(filename)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Serializes XDG_STATE_HOME-dependent tests.
    use std::sync::Mutex;
    static XDG_MUTEX: Mutex<()> = Mutex::new(());


    /// Guard that sets `XDG_STATE_HOME` for the duration of the test and
    /// restores the previous value (or removes it) on drop.
    struct XdgGuard {
        _tmp: TempDir,
        old: Option<String>,
    }

    impl XdgGuard {
        fn new() -> Self {
            let tmp = TempDir::new().expect("tempdir");
            let old = std::env::var("XDG_STATE_HOME").ok();
            std::env::set_var("XDG_STATE_HOME", tmp.path());
            XdgGuard { _tmp: tmp, old }
        }
    }

    impl Drop for XdgGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(v) => std::env::set_var("XDG_STATE_HOME", v),
                None => std::env::remove_var("XDG_STATE_HOME"),
            }
        }
    }

    fn workspace() -> PathBuf {
        PathBuf::from("/test/nixman/workspace")
    }


    #[test]
    fn roundtrip_option_set() {
        let original = StagedChanges {
            changes: vec![StagedChange {
                kind: "option_set".into(),
                option_path: "services.nginx.enable".into(),
                value: "true".into(),
                file: None,
                timestamp: "1000".into(),
            }],
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: StagedChanges = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.changes.len(), 1);
        let c = &restored.changes[0];
        assert_eq!(c.kind, "option_set");
        assert_eq!(c.option_path, "services.nginx.enable");
        assert_eq!(c.value, "true");
        assert!(c.file.is_none());
    }

    #[test]
    fn roundtrip_package_add() {
        let original = StagedChanges {
            changes: vec![StagedChange {
                kind: "package_add".into(),
                option_path: "htop".into(),
                value: String::new(),
                file: Some("packages.nix".into()),
                timestamp: "2000".into(),
            }],
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: StagedChanges = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.changes.len(), 1);
        let c = &restored.changes[0];
        assert_eq!(c.kind, "package_add");
        assert_eq!(c.option_path, "htop");
        assert_eq!(c.value, "");
        assert_eq!(c.file.as_deref(), Some("packages.nix"));
    }

    #[test]
    fn roundtrip_package_remove() {
        let original = StagedChanges {
            changes: vec![StagedChange {
                kind: "package_remove".into(),
                option_path: "firefox".into(),
                value: String::new(),
                file: None,
                timestamp: "3000".into(),
            }],
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: StagedChanges = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.changes.len(), 1);
        let c = &restored.changes[0];
        assert_eq!(c.kind, "package_remove");
        assert_eq!(c.option_path, "firefox");
    }

    #[test]
    fn roundtrip_multiple_changes() {
        let mut changes = StagedChanges::default();
        changes.add_option("a".into(), "1".into());
        changes.add_package_add("b".into(), None);
        changes.add_package_remove("c".into(), Some("f.nix".into()));
        let json = serde_json::to_string(&changes).unwrap();
        let restored: StagedChanges = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.changes.len(), 3);
    }


    #[test]
    fn missing_kind_defaults_to_option_set() {
        let json = r#"{
            "changes": [
                {
                    "option_path": "services.nginx.enable",
                    "value": "true",
                    "file": null,
                    "timestamp": "42"
                }
            ]
        }"#;
        let parsed: StagedChanges = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.changes.len(), 1);
        assert_eq!(parsed.changes[0].kind, "option_set");
    }

    #[test]
    fn missing_kind_in_single_change_defaults_to_option_set() {
        let json = r#"{"option_path":"x","value":"y","file":null,"timestamp":"1"}"#;
        let change: StagedChange = serde_json::from_str(json).unwrap();
        assert_eq!(change.kind, "option_set");
    }


    #[test]
    fn add_option_sets_kind() {
        let mut changes = StagedChanges::default();
        changes.add_option("services.nginx.enable".into(), "true".into());
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].kind, "option_set");
        assert_eq!(changes.changes[0].option_path, "services.nginx.enable");
        assert_eq!(changes.changes[0].value, "true");
    }

    #[test]
    fn add_package_add_sets_kind() {
        let mut changes = StagedChanges::default();
        changes.add_package_add("htop".into(), Some("pkgs.nix".into()));
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].kind, "package_add");
        assert_eq!(changes.changes[0].option_path, "htop");
        assert_eq!(changes.changes[0].value, "");
        assert_eq!(changes.changes[0].file.as_deref(), Some("pkgs.nix"));
    }

    #[test]
    fn add_package_remove_sets_kind() {
        let mut changes = StagedChanges::default();
        changes.add_package_remove("firefox".into(), None);
        assert_eq!(changes.changes.len(), 1);
        assert_eq!(changes.changes[0].kind, "package_remove");
        assert_eq!(changes.changes[0].option_path, "firefox");
        assert_eq!(changes.changes[0].value, "");
        assert!(changes.changes[0].file.is_none());
    }


    #[test]
    fn save_then_load_returns_same_changes() {
        let _lock = XDG_MUTEX.lock().unwrap();
        let _guard = XdgGuard::new();
        let ws = workspace();

        let mut original = StagedChanges::default();
        original.add_option("services.nginx.enable".into(), "true".into());
        original.add_package_add("htop".into(), None);
        original.save(&ws).unwrap();

        let loaded = StagedChanges::load(&ws);
        assert_eq!(loaded.changes.len(), 2);
        assert_eq!(loaded.changes[0].kind, "option_set");
        assert_eq!(loaded.changes[0].option_path, "services.nginx.enable");
        assert_eq!(loaded.changes[1].kind, "package_add");
        assert_eq!(loaded.changes[1].option_path, "htop");
    }

    #[test]
    fn load_nonexistent_file_returns_empty() {
        let _lock = XDG_MUTEX.lock().unwrap();
        let _guard = XdgGuard::new();
        let ws = PathBuf::from("/nonexistent/workspace");
        let loaded = StagedChanges::load(&ws);
        assert!(loaded.changes.is_empty());
    }

    #[test]
    fn save_empty_then_load_returns_empty() {
        let _lock = XDG_MUTEX.lock().unwrap();
        let _guard = XdgGuard::new();
        let ws = workspace();
        let changes = StagedChanges::default();
        changes.save(&ws).unwrap();
        let loaded = StagedChanges::load(&ws);
        assert!(loaded.changes.is_empty());
    }


    #[test]
    fn discard_removes_staging_file() {
        let _lock = XDG_MUTEX.lock().unwrap();
        let _guard = XdgGuard::new();
        let ws = workspace();

        let mut changes = StagedChanges::default();
        changes.add_option("a".into(), "1".into());
        changes.save(&ws).unwrap();

        let path = staging_path(&ws);
        assert!(path.exists(), "staging file should exist before discard");

        StagedChanges::discard(&ws);
        assert!(!path.exists(), "staging file should be gone after discard");
    }

    #[test]
    fn discard_is_idempotent() {
        let _lock = XDG_MUTEX.lock().unwrap();
        let _guard = XdgGuard::new();
        let ws = workspace();
        StagedChanges::discard(&ws);
        StagedChanges::discard(&ws);
    }


    #[test]
    fn add_same_option_path_replaces_not_duplicates() {
        let mut changes = StagedChanges::default();
        changes.add_option("services.nginx.enable".into(), "true".into());
        assert_eq!(changes.count(), 1);
        changes.add_option("services.nginx.enable".into(), "false".into());
        assert_eq!(changes.count(), 1, "should replace, not duplicate");
        assert_eq!(changes.changes[0].value, "false");
    }

    #[test]
    fn add_different_option_paths_creates_separate_entries() {
        let mut changes = StagedChanges::default();
        changes.add_option("a".into(), "1".into());
        changes.add_option("b".into(), "2".into());
        assert_eq!(changes.count(), 2);
    }

    #[test]
    fn different_kinds_same_path_does_not_replace() {
        let mut changes = StagedChanges::default();
        changes.add_option("htop".into(), "true".into());
        changes.add_package_add("htop".into(), None);
        assert_eq!(changes.count(), 2, "different kinds should not replace each other");
    }

    #[test]
    fn add_package_add_same_package_replaces() {
        let mut changes = StagedChanges::default();
        changes.add_package_add("htop".into(), Some("a.nix".into()));
        changes.add_package_add("htop".into(), Some("b.nix".into()));
        assert_eq!(changes.count(), 1);
        assert_eq!(changes.changes[0].file.as_deref(), Some("b.nix"));
    }


    #[test]
    fn is_empty_initial() {
        let changes = StagedChanges::default();
        assert!(changes.is_empty());
    }

    #[test]
    fn is_empty_after_add() {
        let mut changes = StagedChanges::default();
        changes.add_option("a".into(), "1".into());
        assert!(!changes.is_empty());
    }

    #[test]
    fn count_zero_initial() {
        let changes = StagedChanges::default();
        assert_eq!(changes.count(), 0);
    }

    #[test]
    fn count_matches_adds() {
        let mut changes = StagedChanges::default();
        changes.add_option("a".into(), "1".into());
        changes.add_option("b".into(), "2".into());
        changes.add_option("c".into(), "3".into());
        assert_eq!(changes.count(), 3);
    }

    #[test]
    fn load_malformed_file_returns_empty() {
        let _lock = XDG_MUTEX.lock().unwrap();
        let _guard = XdgGuard::new();
        let ws = workspace();
        let path = staging_path(&ws);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not valid json").unwrap();
        let loaded = StagedChanges::load(&ws);
        assert!(loaded.changes.is_empty(), "malformed JSON should yield empty");
    }
}
