//! Flake input inspection.
//!
//! Parses `flake.lock` (Nix lockfile v7) to expose metadata about a flake's
//! direct inputs without invoking `nix` — reads the JSON file directly, which
//! works offline and is fast.
//!
//! # Quick start
//!
//! ```ignore
//! use std::path::Path;
//! use nixman_core::flake;
//!
//! let inputs = flake::metadata::list_inputs(Path::new("/etc/nixos"))?;
//! for input in inputs {
//!     println!("{}: {} @ {}", input.name, input.url, input.rev);
//! }
//! ```

pub mod metadata;
pub mod types;

pub use types::{FlakeError, FlakeInput};
