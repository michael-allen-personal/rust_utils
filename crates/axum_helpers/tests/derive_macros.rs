//! Consumer-perspective compile test for the route derive macros.
//!
//! This is a separate crate whose ONLY dependencies are `axum_helpers` and `macros`.
//! It deliberately does not depend on `serde`, `sqlx`, or `sql_traits` by name — every
//! such type is reached through `axum_helpers`' re-exports. If the derive macros emitted
//! bare `serde::`/`sql_traits::` paths, this crate would fail to compile, so the fact
//! that it builds is the assertion that the generated output is self-contained.
//!
//! Note: the derived `impl` blocks are type-checked whether or not they are ever used,
//! so their mere existence forces every generated path and trait bound to resolve.

use axum_helpers::async_trait::async_trait;
use axum_helpers::sqlx::{self, PgPool};

// serde is reached via the re-export; `#[serde(crate = ...)]` points the derive's
// generated code at the same re-exported path.
#[derive(
    axum_helpers::serde::Serialize,
    axum_helpers::serde::Deserialize,
    macros::BasicCrudRoutes,
)]
#[serde(crate = "axum_helpers::serde")]
struct Widget {
    #[allow(dead_code)]
    id: i64,
}

// Supertrait impls the route derives require. Bodies only need to type-check.
impl axum_helpers::sql_traits::HasPrimaryKey for Widget {
    type PrimaryKey = i64;
}

#[async_trait]
impl axum_helpers::sql_traits::GetLatestRecord for Widget {
    async fn get_latest_record(_pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl axum_helpers::sql_traits::GetRecord for Widget {
    async fn get_record(
        _pool: &PgPool,
        _primary_key: <Self as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl axum_helpers::sql_traits::ListRecords for Widget {
    async fn get_all(_pool: &PgPool) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::InsertSQL for Widget {
    type ReturnType = ();
    async fn insert_sql(self, _pool: &PgPool) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::BulkInsertSQL for Widget {
    type ReturnType = ();
    async fn bulk_insert_sql(
        _pool: &PgPool,
        _records: &[Self],
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::DeleteSQL for Widget {
    type ReturnType = ();
    async fn delete_sql(
        _pool: &PgPool,
        _primary_key: <Self as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

// Concrete check for the route traits without lifetime parameters. The `CreateRoute`
// and `BulkCreateRoute` impls are verified by the compiler through the derive above.
// The `DeserializeOwned` bound is restated because a trait's `where` clause is not
// elaborated into a generic caller's environment. Concrete impls (what the derive emits)
// do not need this — they get the requirement checked at the impl site.
fn assert_routes<T>()
where
    T: axum_helpers::GetLatestRoute
        + axum_helpers::GetRecordRoute
        + axum_helpers::ListRecordsRoute
        + axum_helpers::DeleteRoute,
    <T as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey:
        axum_helpers::serde::de::DeserializeOwned,
{
}

#[test]
fn basic_crud_routes_derive_is_self_contained() {
    assert_routes::<Widget>();
}
