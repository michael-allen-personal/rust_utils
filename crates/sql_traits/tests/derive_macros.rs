//! Consumer-perspective compile test for the `PrimaryKey` derive.
//!
//! This is a separate crate that depends only on `sql_traits` and `macros`. The whole
//! point is that it compiles: the derive's generated `::sql_traits::HasPrimaryKey` path
//! resolves without the use site declaring anything beyond those two crates.

// The struct fields exist only to drive the derive; they are never read directly.
#![allow(dead_code)]

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
