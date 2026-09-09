//! Consumer-perspective test for the `PrimaryKey` and `Update` derives.
//!
//! This is a separate crate, and it names neither `serde` nor `sql_traits`' own
//! dependencies: `serde` is reached through `sql_traits`' re-export. That the file compiles
//! is half the assertion — the derives' generated `::sql_traits::HasPrimaryKey` and
//! `::sql_traits::double_option` paths have to resolve from outside the crate. The `Update`
//! section adds runtime assertions, because a generated `deserialize_with` that resolves but
//! is not actually attached would still compile and would still lose an explicit `null`.

// The struct fields exist only to drive the derive; they are never read directly.
#![allow(dead_code)]

use sql_traits::UpdateFields as _;

// Single primary key -> `PrimaryKey` is the field's type.
#[derive(macros::PrimaryKey)]
struct User {
    #[macros(primary_key)]
    id: i64,
    name: String,
}

// Composite primary key -> `PrimaryKey` is a tuple in declaration order.
#[derive(macros::PrimaryKey)]
struct Membership {
    #[macros(primary_key)]
    user_id: i64,
    #[macros(primary_key)]
    group_id: i64,
}

// A non-`Copy` primary key -> the generated accessor has to clone, not move out of `&self`.
#[derive(macros::PrimaryKey)]
struct ApiKey {
    #[macros(primary_key)]
    token: String,
    label: String,
}

fn assert_has_primary_key<T: sql_traits::HasPrimaryKey>() {}

fn assert_get_record<T: sql_traits::GetRecord>() {}

// `GetRecord`'s argument type is `HasPrimaryKey::PrimaryKey`, so implementing it against a
// derived primary key is what proves the two traits line up.
#[sql_traits::async_trait::async_trait]
impl sql_traits::GetRecord for User {
    async fn get_record(
        _pool: &sql_traits::sqlx::PgPool,
        _primary_key: <Self as sql_traits::HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sql_traits::sqlx::Error> {
        Ok(None)
    }
}

#[test]
fn primary_key_derive_resolves_and_sets_associated_type() {
    assert_has_primary_key::<User>();
    assert_has_primary_key::<Membership>();
    assert_get_record::<User>();

    // The generated associated types are what we expect.
    let _single: <User as sql_traits::HasPrimaryKey>::PrimaryKey = 1_i64;
    let _composite: <Membership as sql_traits::HasPrimaryKey>::PrimaryKey = (1_i64, 2_i64);
}

#[test]
fn primary_key_reads_the_marked_fields_off_the_record() {
    use sql_traits::HasPrimaryKey;

    let user = User {
        id: 7,
        name: "ada".to_string(),
    };
    assert_eq!(user.primary_key(), 7_i64);

    let membership = Membership {
        user_id: 1,
        group_id: 2,
    };
    assert_eq!(membership.primary_key(), (1_i64, 2_i64));
}

#[test]
fn primary_key_clones_a_non_copy_key_and_leaves_the_record_usable() {
    use sql_traits::HasPrimaryKey;

    let api_key = ApiKey {
        token: "sk-123".to_string(),
        label: "ci".to_string(),
    };

    assert_eq!(api_key.primary_key(), "sk-123".to_string());
    // Still readable afterwards: the accessor borrowed, it did not consume the field.
    assert_eq!(api_key.token, "sk-123");
}

// `Record` derive: the body type is the record minus its key fields.
#[derive(Debug, Clone, PartialEq, macros::Record)]
#[macros(body_derive(Debug, PartialEq))]
struct Product {
    #[macros(primary_key)]
    sku: i64,
    title: String,
    tags: Vec<String>,
}

#[test]
fn record_derive_generates_a_body_type_without_the_key() {
    let body = ProductBody {
        title: "widget".to_string(),
        tags: vec!["new".to_string()],
    };

    // Compiles only if ProductBody exists with exactly these fields, and the derive
    // applied Debug + PartialEq to it.
    assert_eq!(
        body,
        ProductBody {
            title: "widget".to_string(),
            tags: vec!["new".to_string()]
        }
    );
}

#[test]
fn record_derive_also_implements_has_primary_key() {
    let product = Product {
        sku: 42,
        title: "widget".to_string(),
        tags: Vec::new(),
    };
    assert_eq!(sql_traits::HasPrimaryKey::primary_key(&product), 42_i64);
}

// Composite key, to prove the body strips every marked field and the tuple is rebuilt
// in declaration order.
#[derive(Debug, Clone, PartialEq, macros::Record)]
#[macros(body_derive(Debug, PartialEq))]
struct Enrollment {
    #[macros(primary_key)]
    student_id: i64,
    #[macros(primary_key)]
    course_id: i64,
    grade: String,
}

#[test]
fn record_round_trips_through_key_and_body() {
    use sql_traits::{HasPrimaryKey, HasRequestBody, RequestBody};

    let original = Product {
        sku: 42,
        title: "widget".to_string(),
        tags: vec!["new".to_string()],
    };

    // record -> (key, body) -> record is identity. One assertion over the accessor, both
    // traits and both From impls; a field mapped to the wrong slot fails here.
    let key = original.primary_key();
    let body = ProductBody::from(original.clone());
    let round_tripped: Product = (key, body).into();
    assert_eq!(original, round_tripped);

    // The same trip spelled through each of the other three entry points.
    assert_eq!(ProductBody::from(original.clone()).with_key(42), original);
    assert_eq!(
        Product::from_request_body(ProductBody::from(original.clone()), 42),
        original
    );
}

// A non-`Copy` key, to prove the generated accessor and assembly clone rather than
// trying to move out of `&self`.
#[derive(Debug, Clone, PartialEq, macros::Record)]
#[macros(body_derive(Debug, PartialEq))]
struct Session {
    #[macros(primary_key)]
    token: String,
    user_id: i64,
}

#[test]
fn record_round_trips_a_non_copy_key() {
    use sql_traits::{HasPrimaryKey, HasRequestBody};

    let original = Session {
        token: "sk-123".to_string(),
        user_id: 7,
    };

    assert_eq!(original.primary_key(), "sk-123".to_string());
    // The record is still usable: the accessor borrowed rather than consuming.
    assert_eq!(original.token, "sk-123");

    let body = SessionBody::from(original.clone());
    assert_eq!(
        Session::from_request_body(body, "sk-123".to_string()),
        original
    );
}

#[test]
fn record_round_trips_a_composite_key() {
    use sql_traits::{HasPrimaryKey, HasRequestBody};

    let original = Enrollment {
        student_id: 1,
        course_id: 2,
        grade: "A".to_string(),
    };

    assert_eq!(original.primary_key(), (1_i64, 2_i64));

    let body = EnrollmentBody::from(original.clone());
    assert_eq!(
        body,
        EnrollmentBody {
            grade: "A".to_string()
        }
    );
    assert_eq!(Enrollment::from_request_body(body, (1, 2)), original);
}

// --- `macros::Update` ---------------------------------------------------------------
//
// The end-to-end assertion for a partial update: the derive's generated
// `::sql_traits::double_option` path has to resolve from a crate that never names it, and
// the three states have to survive the trip from JSON through `apply` onto a record.

// `Update` deliberately does not emit `HasPrimaryKey` — it is a supertrait of
// `HasUpdateFields`, so the type needs `PrimaryKey` (or `Record`) alongside it. Deriving
// `Update` on its own fails at the generated impl with an unsatisfied `HasPrimaryKey`
// bound, which is the intended pairing made visible rather than a silent gap.
#[derive(Debug, PartialEq, sql_traits::serde::Serialize, macros::PrimaryKey, macros::Update)]
#[macros(update_derive(sql_traits::serde::Deserialize))]
#[serde(crate = "sql_traits::serde")]
struct Widget {
    #[macros(primary_key)]
    id: i64,
    name: String,
    /// Nullable, so the generated field is doubly wrapped and carries the helper.
    qty: Option<i32>,
}

fn cog() -> Widget {
    Widget {
        id: 3,
        name: "cog".to_string(),
        qty: Some(9),
    }
}

fn patch(json: &str) -> WidgetUpdate {
    serde_json::from_str(json).expect("valid json")
}

#[test]
fn the_generated_update_type_omits_the_primary_key() {
    // `WidgetUpdate` has no `id` field to name — this is a compile assertion.
    let updated = patch(r#"{"name": "sprocket"}"#).apply(cog());
    assert_eq!(updated.id, 3, "the key can only come from the path");
}

#[test]
fn an_absent_field_leaves_the_column_alone() {
    assert_eq!(patch("{}").apply(cog()), cog());
}

#[test]
fn a_named_field_is_written() {
    assert_eq!(
        patch(r#"{"name": "sprocket"}"#).apply(cog()).name,
        "sprocket"
    );
}

#[test]
fn an_explicit_null_clears_a_nullable_column() {
    assert_eq!(
        patch(r#"{"qty": null}"#).apply(cog()).qty,
        None,
        "the generated deserialize_with is what keeps this apart from an absent key"
    );
}

#[test]
fn a_nullable_column_can_still_be_set_to_a_value() {
    assert_eq!(patch(r#"{"qty": 4}"#).apply(cog()).qty, Some(4));
}

#[test]
fn the_generated_is_empty_reports_a_patch_that_changes_nothing() {
    assert!(patch("{}").is_empty());
    assert!(!patch(r#"{"qty": null}"#).is_empty());
    assert!(!patch(r#"{"name": "sprocket"}"#).is_empty());
}
