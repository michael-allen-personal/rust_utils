//! Exercises `str_enum!` the way a downstream crate does.
//!
//! Files under `tests/` compile as their own crates linking `generic_helpers`
//! externally, so name resolution here matches a real consumer's. That is the
//! point: an in-crate `#[cfg(test)] mod tests` resolves every path in the
//! expansion locally and so cannot catch a missing `#[macro_export]` or an
//! unqualified path leaking out of the macro.
//!
//! Deliberately imports *only* the macro. No `FromStr`, `fmt`, `type_name` or
//! `ParsingError` in scope — if the expansion needs any of them unqualified,
//! this file stops compiling.

use generic_helpers::str_enum;

str_enum! {
    /// Enum-level attributes pass through to the generated type.
    pub enum ExerciseMuscleRole {
        Primary => "Primary",
        Secondary => "Secondary",
        Stabilizer => "Stabilizer",
        /// Variant-level attributes pass through too, which is what lets callers
        /// attach `#[serde(rename = "...")]` without the macro depending on serde.
        DynamicStabilizer => "Dynamic Stabilizer",
        AntagonistStabilizer => "Antagonist Stabilizer",
    }
}

str_enum! {
    pub enum ExerciseForceType {
        Push => "Push" | "press",
        Pull => "Pull" | "row",
    }
}

#[test]
fn round_trips_through_display() {
    assert!(
        ExerciseMuscleRole::ALL
            .iter()
            .all(|role| role.as_str().parse::<ExerciseMuscleRole>().ok() == Some(*role))
    );
}

#[test]
fn ignores_case_and_separators() {
    let inputs = [
        "dynamic stabilizer",
        "DynamicStabilizer",
        "dynamic-stabilizer",
        "dynamic_stabilizer",
        "DYNAMIC STABILIZER",
    ];

    assert!(
        inputs
            .into_iter()
            .all(|s| s.parse().ok() == Some(ExerciseMuscleRole::DynamicStabilizer))
    );
}

#[test]
fn expected_lists_every_variant() {
    assert_eq!(
        ExerciseMuscleRole::EXPECTED,
        "[Primary, Secondary, Stabilizer, Dynamic Stabilizer, Antagonist Stabilizer]"
    );
}

#[test]
fn parses_aliases_but_displays_the_canonical_string() {
    assert!("press".parse().ok() == Some(ExerciseForceType::Push));
    assert!("ROW".parse().ok() == Some(ExerciseForceType::Pull));
    assert_eq!(ExerciseForceType::Push.to_string(), "Push");
}

#[test]
fn rejects_unknown_values_and_names_the_type() {
    let err = "Squat"
        .parse::<ExerciseForceType>()
        .expect_err("'Squat' is not a force type");
    let message = err.to_string();

    assert!(message.contains("Squat"), "{message}");
    assert!(message.contains("ExerciseForceType"), "{message}");
    assert!(message.contains("[Push, Pull]"), "{message}");
}

#[test]
fn converts_from_both_str_and_string() {
    // `TryFrom`/`TryInto` are in the 2024 prelude, so a consumer needs no import
    // for either of these — the same property the rest of this file relies on.
    assert!(ExerciseForceType::try_from("press").ok() == Some(ExerciseForceType::Push));
    assert!(ExerciseForceType::try_from(String::from("ROW")).ok() == Some(ExerciseForceType::Pull));
}

#[test]
fn try_into_infers_the_target_type() {
    let role: Option<ExerciseMuscleRole> = "dynamic-stabilizer".try_into().ok();

    assert!(role == Some(ExerciseMuscleRole::DynamicStabilizer));
}

#[test]
fn try_from_string_reports_the_rejected_value() {
    // The owned overload moves the input into the error rather than copying it,
    // so this is what proves the value still reaches the message.
    let err = ExerciseForceType::try_from(String::from("Squat"))
        .expect_err("'Squat' is not a force type");
    let message = err.to_string();

    assert!(message.contains("Squat"), "{message}");
    assert!(message.contains("ExerciseForceType"), "{message}");
    assert!(message.contains("[Push, Pull]"), "{message}");
}
