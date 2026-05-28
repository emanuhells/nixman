//! Unit tests for generation types, sorting, and error formatting.
//!
//! Because listing generations requires `/nix/var/nix/profiles/` (not
//! available in every test environment), these tests focus on the in-memory
//! data structures and pure helper logic that underpin the module.

use chrono::Utc;

use crate::generations::types::{GcResult, Generation, GenerationDiff, GenerationError};

// Helpers

/// Build a mock `Generation` with the given number and `is_current` flag.
fn mock_gen(number: u32, is_current: bool) -> Generation {
    Generation {
        number,
        date: Utc::now(),
        nixos_version: format!("23.11.{}", number),
        kernel_version: "6.1.0".to_string(),
        path: std::path::PathBuf::from(format!("/nix/store/fake-gen-{}", number)),
        is_current,
    }
}

// Sorting

/// Generations should be sortable by number descending (most-recent first),
/// matching the order returned by `list::all()`.
#[test]
fn test_generation_sort_descending() {
    let mut gens = vec![mock_gen(1, false), mock_gen(5, false), mock_gen(3, false)];
    gens.sort_by(|a, b| b.number.cmp(&a.number));

    assert_eq!(gens[0].number, 5);
    assert_eq!(gens[1].number, 3);
    assert_eq!(gens[2].number, 1);
}

/// A single-element list is already in order.
#[test]
fn test_generation_sort_single() {
    let mut gens = vec![mock_gen(42, true)];
    gens.sort_by(|a, b| b.number.cmp(&a.number));
    assert_eq!(gens[0].number, 42);
}

// is_current flag

/// Only the generation flagged with `is_current = true` should be marked
/// as the active one.
#[test]
fn test_generation_is_current_flag() {
    let gens = vec![
        mock_gen(10, false),
        mock_gen(11, false),
        mock_gen(12, true), // active
    ];

    let current: Vec<&Generation> = gens.iter().filter(|g| g.is_current).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].number, 12);
}

/// A list of generations with no current generation should produce zero hits.
#[test]
fn test_generation_no_current() {
    let gens = vec![mock_gen(1, false), mock_gen(2, false)];
    assert!(gens.iter().all(|g| !g.is_current));
}

// NixOS version string

/// The `nixos_version` field is accessible and stores the expected string.
#[test]
fn test_generation_nixos_version() {
    let gen = mock_gen(7, false);
    assert_eq!(gen.nixos_version, "23.11.7");
}

// GenerationDiff

/// A diff with added, removed, and changed packages should store all lists.
#[test]
fn test_generation_diff_fields() {
    let diff = GenerationDiff {
        added_packages: vec!["ripgrep-14.0.0".into(), "bat-0.24.0".into()],
        removed_packages: vec!["grep-3.8".into()],
        changed_packages: vec![
            ("neovim".into(), "0.9.0".into(), "0.9.5".into()),
        ],
    };

    assert_eq!(diff.added_packages.len(), 2);
    assert_eq!(diff.removed_packages.len(), 1);
    assert_eq!(diff.changed_packages.len(), 1);
    assert_eq!(diff.changed_packages[0].0, "neovim");
    assert_eq!(diff.changed_packages[0].1, "0.9.0");
    assert_eq!(diff.changed_packages[0].2, "0.9.5");
}

/// An empty diff is valid and has zero entries in every list.
#[test]
fn test_generation_diff_empty() {
    let diff = GenerationDiff {
        added_packages: vec![],
        removed_packages: vec![],
        changed_packages: vec![],
    };
    assert!(diff.added_packages.is_empty());
    assert!(diff.removed_packages.is_empty());
    assert!(diff.changed_packages.is_empty());
}

// GcResult

/// `GcResult` should store the freed bytes and deleted generation numbers.
#[test]
fn test_gc_result_fields() {
    let result = GcResult {
        freed_bytes: 1_234_567_890,
        deleted_generations: vec![1, 2, 3, 4, 5],
    };

    assert_eq!(result.freed_bytes, 1_234_567_890);
    assert_eq!(result.deleted_generations.len(), 5);
    assert_eq!(result.deleted_generations[0], 1);
    assert_eq!(result.deleted_generations[4], 5);
}

/// A GcResult with no deleted generations (garbage collected without pruning)
/// should report zero deleted generations.
#[test]
fn test_gc_result_no_deleted_gens() {
    let result = GcResult {
        freed_bytes: 512,
        deleted_generations: vec![],
    };
    assert!(result.deleted_generations.is_empty());
    assert_eq!(result.freed_bytes, 512);
}

// GenerationError — Display

/// Every `GenerationError` variant must produce a non-empty display string.
#[test]
fn test_generation_error_display() {
    use std::io;

    let variants: Vec<GenerationError> = vec![
        GenerationError::IoError(io::Error::new(io::ErrorKind::NotFound, "not found")),
        GenerationError::ParseError("unexpected format".into()),
        GenerationError::CommandFailed {
            exit_code: 1,
            stderr: "error output".into(),
        },
        GenerationError::GenerationNotFound(99),
    ];

    for variant in &variants {
        let msg = variant.to_string();
        assert!(!msg.is_empty(), "Display for variant should produce non-empty string");
    }
}

/// `GenerationNotFound` includes the generation number in its message.
#[test]
fn test_generation_error_not_found_message() {
    let err = GenerationError::GenerationNotFound(42);
    assert!(err.to_string().contains("42"));
}

/// `CommandFailed` includes the exit code in its message.
#[test]
fn test_generation_error_command_failed_message() {
    let err = GenerationError::CommandFailed {
        exit_code: 126,
        stderr: "permission denied".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("126"));
    assert!(msg.contains("permission denied"));
}

// Serialization round-trip

/// A `Generation` can be serialised to JSON and deserialised back unchanged.
#[test]
fn test_generation_serde_round_trip() {
    let original = mock_gen(3, true);
    let json = serde_json::to_string(&original).expect("serialize");
    let restored: Generation = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.number, original.number);
    assert_eq!(restored.nixos_version, original.nixos_version);
    assert_eq!(restored.kernel_version, original.kernel_version);
    assert_eq!(restored.is_current, original.is_current);
    assert_eq!(restored.path, original.path);
}
