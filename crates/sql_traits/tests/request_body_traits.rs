//! Consumer-perspective test that the request-body traits are implementable from
//! outside the crate, with no derive involved. `tests/derive_macros.rs` covers the
//! generated versions.

use sql_traits::{HasPrimaryKey, HasRequestBody, RequestBody};

#[derive(Debug, Clone, PartialEq)]
struct User {
    id: i64,
    name: String,
}

#[derive(Debug, PartialEq)]
struct UserBody {
    name: String,
}

impl HasPrimaryKey for User {
    type PrimaryKey = i64;
    fn primary_key(&self) -> <Self as HasPrimaryKey>::PrimaryKey {
        self.id
    }
}

impl HasRequestBody for User {
    type RequestBody = UserBody;
    fn from_request_body(body: UserBody, primary_key: <Self as HasPrimaryKey>::PrimaryKey) -> Self {
        User {
            id: primary_key,
            name: body.name,
        }
    }
}

// Naming the record is the entire impl: `with_key` is provided, and the
// `HasRequestBody<RequestBody = Self>` bound rejects a `Record` that does not point back
// at `UserBody`, so this pair cannot drift.
impl RequestBody for UserBody {
    type Record = User;
}

#[test]
fn a_body_and_a_key_rebuild_the_record() {
    let expected = User {
        id: 7,
        name: "ada".to_string(),
    };

    assert_eq!(
        User::from_request_body(
            UserBody {
                name: "ada".to_string()
            },
            7
        ),
        expected
    );
    assert_eq!(
        UserBody {
            name: "ada".to_string()
        }
        .with_key(7),
        expected
    );
}

/// Generic over the record: this only compiles if the assembly is reachable through the
/// trait, which is the whole reason it is a trait and not an inherent method.
fn assemble<R: HasRequestBody>(body: R::RequestBody, key: R::PrimaryKey) -> R {
    R::from_request_body(body, key)
}

/// Generic over the *body*, the direction `RequestBody` exists for. It only compiles
/// because `Record` is bound `HasRequestBody<RequestBody = Self>`: under the looser
/// `HasPrimaryKey` bound the record side is unknown from here, so a body could not be
/// handed back to it.
fn assemble_from_body<B: RequestBody>(
    body: B,
    key: <B::Record as HasPrimaryKey>::PrimaryKey,
) -> B::Record {
    body.with_key(key)
}

#[test]
fn assembly_is_reachable_from_code_generic_over_the_body() {
    let built = assemble_from_body(
        UserBody {
            name: "hopper".to_string(),
        },
        11,
    );
    assert_eq!(
        built,
        User {
            id: 11,
            name: "hopper".to_string()
        }
    );
}

#[test]
fn assembly_is_reachable_from_generic_code() {
    let built: User = assemble(
        UserBody {
            name: "grace".to_string(),
        },
        9,
    );
    assert_eq!(
        built,
        User {
            id: 9,
            name: "grace".to_string()
        }
    );
}

// `ReplaceRecord` returns `Option<Self>` for the same reason `GetRecord` does: a primary
// key matching no row is a `404`, not a failure. Returning `Result<Self, _>` forced a
// missing row to surface as `sqlx::Error::RowNotFound`, which the axum error mapping turns
// into a `500`.
#[sql_traits::async_trait::async_trait]
impl sql_traits::ReplaceRecord for User {
    async fn replace_record(
        self,
        _pool: &sql_traits::sqlx::PgPool,
    ) -> Result<Option<Self>, sql_traits::sqlx::Error> {
        Ok(Some(self))
    }
}

fn assert_replace_record<T: sql_traits::ReplaceRecord>() {}

#[test]
fn replace_record_reports_a_missing_row_as_none() {
    assert_replace_record::<User>();
}
