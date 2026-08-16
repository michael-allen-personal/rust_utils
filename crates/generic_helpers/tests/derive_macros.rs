//! Consumer-perspective compile test for the `MaxVecCapacity` derive.
//!
//! This is a separate crate that depends only on `generic_helpers` and `macros`. The
//! whole point is that it compiles: the derive's generated
//! `::generic_helpers::MaxVecCapacity` path resolves without the use site naming the
//! trait at all. The derive previously shipped emitting a path to a crate that lived in
//! another repo, which no consumer could resolve — nothing in this workspace noticed,
//! because the only thing that catches it is a use site outside the crate itself.

// The struct fields exist only to give the types a size; they are never read directly.
#![allow(dead_code)]

#[derive(macros::MaxVecCapacity)]
struct QuantityRecord {
    value: f64,
    unit: u32,
}

#[derive(macros::MaxVecCapacity)]
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
