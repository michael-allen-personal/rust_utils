//! Consumer-perspective test that the update-fields traits are implementable from outside
//! the crate, with no derive involved. `tests/derive_macros.rs` covers the generated
//! versions.
//!
//! The three states a partial update has to distinguish are the whole point of the pair,
//! so each gets its own test: a field left out, a field set to a value, and a nullable
//! field set explicitly to null.

use sql_traits::{HasPrimaryKey, HasUpdateFields, UpdateFields};

#[derive(Debug, PartialEq)]
struct User {
    id: i64,
    name: String,
    /// Nullable, so a patch has to be able to clear it as well as set it.
    nickname: Option<String>,
}

/// `User`'s non-key fields, each wrapped so absent is distinguishable from set. `nickname`
/// is doubly wrapped because it is itself nullable: `Some(None)` clears the column.
#[derive(Debug, PartialEq)]
struct UserUpdate {
    name: Option<String>,
    nickname: Option<Option<String>>,
}

impl HasPrimaryKey for User {
    type PrimaryKey = i64;
    fn primary_key(&self) -> <Self as HasPrimaryKey>::PrimaryKey {
        self.id
    }
}

impl HasUpdateFields for User {
    type UpdateFields = UserUpdate;

    fn apply_update_fields(mut record: Self, fields: UserUpdate) -> Self {
        if let Some(name) = fields.name {
            record.name = name;
        }
        if let Some(nickname) = fields.nickname {
            record.nickname = nickname;
        }
        record
    }
}

// Naming the record plus `is_empty` is the entire impl: `apply` is provided, and the
// `HasUpdateFields<UpdateFields = Self>` bound rejects a `Record` that does not point back
// at `UserUpdate`, so this pair cannot drift.
impl UpdateFields for UserUpdate {
    type Record = User;

    fn is_empty(&self) -> bool {
        self.name.is_none() && self.nickname.is_none()
    }
}

fn ada() -> User {
    User {
        id: 7,
        name: "ada".to_string(),
        nickname: Some("lovelace".to_string()),
    }
}

#[test]
fn a_set_field_is_written_to_the_record() {
    let updated = UserUpdate {
        name: Some("grace".to_string()),
        nickname: None,
    }
    .apply(ada());

    assert_eq!(updated.name, "grace");
}

#[test]
fn an_absent_field_leaves_the_record_alone() {
    let updated = UserUpdate {
        name: None,
        nickname: None,
    }
    .apply(ada());

    assert_eq!(updated, ada(), "an empty patch must change nothing");
}

#[test]
fn an_explicit_null_clears_a_nullable_column() {
    let updated = UserUpdate {
        name: None,
        nickname: Some(None),
    }
    .apply(ada());

    assert_eq!(updated.nickname, None);
    assert_eq!(
        updated.name, "ada",
        "clearing one field must not touch another"
    );
}

#[test]
fn is_empty_distinguishes_a_patch_with_nothing_set() {
    assert!(
        UserUpdate {
            name: None,
            nickname: None
        }
        .is_empty()
    );
    assert!(
        !UserUpdate {
            name: None,
            nickname: Some(None)
        }
        .is_empty(),
        "an explicit null is a change, not an absent field"
    );
}

#[test]
fn the_primary_key_is_never_part_of_the_update_fields() {
    // `UserUpdate` has no `id` field to set; the key comes from the path. This is a compile
    // assertion as much as a runtime one.
    let updated = UserUpdate {
        name: Some("grace".to_string()),
        nickname: Some(None),
    }
    .apply(ada());

    assert_eq!(updated.primary_key(), 7);
}

// `UpdateRecord` takes its fields type from `HasUpdateFields` rather than declaring one of
// its own, so the pair above is the single source of truth for what a partial update is.
// The `Option` return is what lets a route answer `404` for a key that matches no row:
// with `Result<Self, _>` a missing row arrives as `sqlx::Error::RowNotFound` and turns
// into a `500`.
#[sql_traits::async_trait::async_trait]
impl sql_traits::UpdateRecord for User {
    async fn update_record(
        _pool: &sql_traits::sqlx::PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
        update_fields: <Self as HasUpdateFields>::UpdateFields,
    ) -> Result<Option<Self>, sql_traits::sqlx::Error> {
        Ok(Some(update_fields.apply(User {
            id: primary_key,
            name: "ada".to_string(),
            nickname: None,
        })))
    }
}

fn assert_update_record<T: sql_traits::UpdateRecord>() {}

#[test]
fn update_record_sources_its_fields_type_from_has_update_fields() {
    assert_update_record::<User>();
    let _: <User as HasUpdateFields>::UpdateFields = UserUpdate {
        name: None,
        nickname: None,
    };
}
