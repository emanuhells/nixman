//! nixman-core — Domain logic for NixOS configuration management.
//!
//! This library provides all core functionality for nixman independent
//! of any GUI or CLI framework.

#![allow(
    clippy::collapsible_match,
    clippy::single_match,
    clippy::useless_format,
    clippy::needless_borrow,
    clippy::doc_overindented_list_items,
    clippy::doc_lazy_continuation,
    clippy::manual_map,
    clippy::io_other_error,
    clippy::needless_lifetimes,
    clippy::write_with_newline,
)]

pub mod preflight;
pub mod workspace;
pub mod nix_parser;
pub mod intent;
pub mod config;
pub mod options;
pub mod builder;
pub mod generations;
pub mod packages;
pub mod services;
pub mod privilege;
pub mod watcher;
pub mod git;
pub mod flake;
