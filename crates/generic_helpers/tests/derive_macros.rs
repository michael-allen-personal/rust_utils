//! Consumer-perspective compile test for the `MaxVecCapacity` derive.
//!
//! This is a separate crate whose only dependency is `generic_helpers`. The derive is
//! reached through that crate's re-export, and the generated
//! `::generic_helpers::MaxVecCapacity` path has to resolve from outside the crate, so the
//! fact that this file builds is the assertion.

// The struct fields exist only to give the types a size; they are never read directly.
#![allow(dead_code)]

#[derive(generic_helpers::MaxVecCapacity)]
struct QuantityRecord {
    value: f64,
    unit: u32,
}

#[derive(generic_helpers::MaxVecCapacity)]
struct Workout {
    duration: f64,
    energy_burned: f64,
    started_at: u64,
}

// The trait is named only through its fully-qualified path, never imported. If the
// derive emitted a relative path, or the root re-export moved under `files::`, this
// stops compiling.
fn assert_max_vec_capacity<T: generic_helpers::MaxVecCapacity>() {}

#[test]
fn derive_resolves_for_a_consumer() {
    assert_max_vec_capacity::<QuantityRecord>();
    assert_max_vec_capacity::<Workout>();
}
