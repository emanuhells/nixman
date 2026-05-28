//! Unit tests for the options search and cache modules.
//!
//! These tests exercise `search::query` with mock in-memory `OptionIndex`
//! values and verify the cache hash computation and round-trip.

use chrono::Utc;
use tempfile::TempDir;

use crate::options::{
    cache,
    search::query,
    types::{OptionIndex, OptionMeta, OptionType},
};

// Helpers

fn meta(path: &str, description: &str) -> OptionMeta {
    OptionMeta {
        path: path.to_string(),
        option_type: OptionType::Bool,
        default: None,
        description: description.to_string(),
        declared_in: String::new(),
        example: None,
    }
}

fn make_index(opts: Vec<OptionMeta>) -> OptionIndex {
    OptionIndex {
        options: opts,
        flake_lock_hash: "test-hash".to_string(),
        built_at: Utc::now(),
        nixpkgs_rev: "abc123".to_string(),
    }
}

// Search — query

/// Searching "nginx" should find `services.nginx.*` options.
#[test]
fn test_search_exact_match() {
    let idx = make_index(vec![
        meta("services.nginx.enable", "Enable the nginx web server"),
        meta("services.nginx.package", "The nginx package"),
        meta("services.nginx.virtualHosts", "Virtual hosts config"),
        meta("services.openssh.enable", "Enable openssh"),
    ]);

    let results = query(&idx, "services.nginx.enable", 10);
    // Exact path match → score 100 → first result.
    assert!(!results.is_empty());
    assert_eq!(results[0].path, "services.nginx.enable");
}

/// A partial query "nginx" should match all `services.nginx.*` paths.
#[test]
fn test_search_partial_match() {
    let idx = make_index(vec![
        meta("services.nginx.enable", ""),
        meta("services.nginx.package", ""),
        meta("services.openssh.enable", ""),
    ]);

    let results = query(&idx, "nginx", 10);
    assert_eq!(results.len(), 2);
    // Both nginx options must be present.
    let paths: Vec<&str> = results.iter().map(|o| o.path.as_str()).collect();
    assert!(paths.contains(&"services.nginx.enable"));
    assert!(paths.contains(&"services.nginx.package"));
    // openssh is not in results.
    assert!(!paths.contains(&"services.openssh.enable"));
}

/// Searching "fire" should find `networking.firewall.*` via path contains.
#[test]
fn test_search_partial_match_firewall() {
    let idx = make_index(vec![
        meta("networking.firewall.enable", ""),
        meta("networking.firewall.allowedTCPPorts", ""),
        meta("networking.hostName", ""),
    ]);

    let results = query(&idx, "fire", 10);
    assert_eq!(results.len(), 2);
    let paths: Vec<&str> = results.iter().map(|o| o.path.as_str()).collect();
    assert!(paths.contains(&"networking.firewall.enable"));
    assert!(paths.contains(&"networking.firewall.allowedTCPPorts"));
}

/// An empty query returns the first `limit` results without scoring.
#[test]
fn test_search_empty_query() {
    let idx = make_index(vec![
        meta("a.opt", ""),
        meta("b.opt", ""),
        meta("c.opt", ""),
    ]);

    let results = query(&idx, "", 10);
    // All three returned (limit > count).
    assert_eq!(results.len(), 3);

    // Empty query with limit=2 returns only 2.
    let limited = query(&idx, "", 2);
    assert_eq!(limited.len(), 2);
}

/// Empty query with limit=0 returns nothing.
#[test]
fn test_search_empty_query_zero_limit() {
    let idx = make_index(vec![meta("a", "")]);
    assert!(query(&idx, "", 0).is_empty());
}

/// A query with no match returns an empty vector.
#[test]
fn test_search_no_match() {
    let idx = make_index(vec![meta("services.nginx.enable", "")]);
    assert!(query(&idx, "zzz_no_match_xyz", 10).is_empty());
}

/// Search is case-insensitive.
#[test]
fn test_search_case_insensitive() {
    let idx = make_index(vec![meta("Services.Nginx.Enable", "")]);
    assert_eq!(query(&idx, "nginx", 10).len(), 1);
    assert_eq!(query(&idx, "NGINX", 10).len(), 1);
    assert_eq!(query(&idx, "services", 10).len(), 1);
}

/// Description-only match is included with a lower score than path matches.
#[test]
fn test_search_description_match() {
    let idx = make_index(vec![
        meta("some.option", "Enables the nginx web server"),
        meta("services.nginx.enable", ""),
    ]);

    let results = query(&idx, "nginx", 10);
    assert_eq!(results.len(), 2);
    // Path match scores higher than description match.
    assert_eq!(results[0].path, "services.nginx.enable");
    assert_eq!(results[1].path, "some.option");
}

/// Prefix-match scores higher than contains-match.
#[test]
fn test_search_prefix_beats_contains() {
    let idx = make_index(vec![
        meta("services.foo.enable", ""), // "foo" contained in path
        meta("foo.bar.setting", ""),     // "foo" at start → prefix
    ]);

    let results = query(&idx, "foo", 10);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].path, "foo.bar.setting"); // higher score
}

// Cache — hash computation

/// Two files with identical content must produce the same SHA-256 hash.
#[test]
fn test_cache_hash_computation() {
    let dir = TempDir::new().unwrap();
    let content = b"{ \"nodes\": {}, \"root\": \"root\", \"version\": 7 }";

    // Write the same content to two different flake dirs.
    let dir_a = dir.path().join("flake_a");
    let dir_b = dir.path().join("flake_b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::write(dir_a.join("flake.lock"), content).unwrap();
    std::fs::write(dir_b.join("flake.lock"), content).unwrap();

    let hash_a = cache::hash_flake_lock(&dir_a).expect("hash_a should succeed");
    let hash_b = cache::hash_flake_lock(&dir_b).expect("hash_b should succeed");

    assert_eq!(hash_a, hash_b, "identical content must produce identical hash");
    // The hash should be a non-empty hex string.
    assert!(!hash_a.is_empty());
    assert!(hash_a.chars().all(|c| c.is_ascii_hexdigit()));
}

/// Different file contents must produce different hashes.
#[test]
fn test_cache_hash_different_content() {
    let dir = TempDir::new().unwrap();

    let dir_a = dir.path().join("a");
    let dir_b = dir.path().join("b");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::write(dir_a.join("flake.lock"), b"content A").unwrap();
    std::fs::write(dir_b.join("flake.lock"), b"content B").unwrap();

    let hash_a = cache::hash_flake_lock(&dir_a).expect("hash_a");
    let hash_b = cache::hash_flake_lock(&dir_b).expect("hash_b");

    assert_ne!(hash_a, hash_b, "different content must produce different hashes");
}

/// `hash_flake_lock` returns an error when `flake.lock` is absent.
#[test]
fn test_cache_hash_missing_file() {
    let dir = TempDir::new().unwrap();
    // No flake.lock created — should return an error.
    let result = cache::hash_flake_lock(dir.path());
    assert!(result.is_err(), "missing flake.lock should produce an error");
}
