//! Provides statically linkable OR-Tools C++ libraries.
//!
//! This crate contains no Rust API worth speaking of. Its entire job is the
//! build script: make a usable OR-Tools tree appear on disk and hand its paths
//! to dependent crates through cargo's `links` metadata.
//!
//! A dependent crate reads them in *its own* build script:
//!
//! ```no_run
//! let include = std::env::var("DEP_ORTOOLS_INCLUDE").unwrap();
//! let lib     = std::env::var("DEP_ORTOOLS_LIB").unwrap();
//! ```
//!
//! See `build.rs` for the resolution order and the environment overrides.

/// OR-Tools version this crate provides.
pub const ORTOOLS_VERSION: &str = env!("ORTOOLS_VERSION");

/// Root of the resolved OR-Tools tree, as of build time.
pub const ORTOOLS_ROOT: &str = env!("ORTOOLS_RESOLVED_ROOT");
