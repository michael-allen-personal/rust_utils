//! Small shared helpers. Two unrelated jobs, both needed by more than one
//! consumer:
//!
//! - [`str_enum!`] — declares a fieldless enum plus its string round-trip.
//! - [`buf_reader_from_path`] / [`MaxVecCapacity`] — file helpers for the
//!   export-parsing CLIs.
//!
//! Keep the dependency list to `error_set` alone, and keep additions applicable
//! to more than one consumer — otherwise it belongs in the specific crate.
//! Consumers pull this in for one small helper apiece, so anything heavier lands
//! in their build for no benefit. The generic name is not licence to grow a junk
//! drawer.

pub mod errors;
mod files;
mod string_enum;

// Re-exported at the crate root, not under `files::`, because the `MaxVecCapacity`
// derive in `macros` emits `::generic_helpers::MaxVecCapacity`. Both the crate name
// and this root re-export are load-bearing across repos: moving either breaks every
// derive site with an error that points at the derive rather than at the change.
pub use files::*;

/// Implementation detail of [`str_enum!`]. Not public API: it exists only so
/// the macro's expansion has an absolute path to reach, and it is exempt from
/// this crate's semver.
#[doc(hidden)]
pub mod __private {
    pub use crate::string_enum::normalized_eq;
}
