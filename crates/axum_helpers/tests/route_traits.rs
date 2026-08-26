//! Compile test that every route trait's handler is actually mountable on an axum `Router`.
//!
//! Implementing a route trait and *using* it are separate checks. A type can satisfy a
//! route trait's bounds and still be rejected by `Router::route`, because that is where
//! axum's `Handler` requirements are enforced: `DeserializeOwned` on whatever `Path`
//! extracts, `IntoResponse` on the return, `Send + 'static` throughout. `v0.7.0` tightened
//! several traits precisely because a bad impl compiled and only failed at the mount site
//! with an opaque `Handler` error, so mounting is the check that matches the failure mode.
//!
//! This also covers the `*Where` family (`GetRecordWhereRoute`, `ListRecordsWhereRoute`,
//! `DeleteRecordsWhereRoute`), which the derive macros cannot emit — their `PathParams`
//! associated type has to be chosen by the implementor — and which therefore has no
//! coverage from `derive_macros.rs`.
//!
//! Like `derive_macros.rs`, this crate reaches `axum`, `serde`, `sqlx`, and `sql_traits`
//! only through `axum_helpers`' re-exports, so it doubles as a check that the re-export
//! surface is enough to write a handler against.

// Fields exist to make the types realistic; the test never reads them.
#![allow(dead_code)]

use axum_helpers::async_trait::async_trait;
use axum_helpers::axum::{
    Router,
    routing::{delete, get, post},
};
use axum_helpers::sql_traits::{
    BulkInsertSQL, DeleteRecordsWhere, DeleteSQL, GetLatestRecord, GetRecord, GetRecordWhere,
    HasPrimaryKey, InsertSQL, ListRecords, ListRecordsWhere,
};
use axum_helpers::sqlx::{self, PgPool};
use axum_helpers::{
    BulkCreateRoute, CreateRoute, DeleteRecordsWhereRoute, DeleteRoute, GetLatestRoute,
    GetRecordRoute, GetRecordWhereRoute, ListRecordsRoute, ListRecordsWhereRoute,
};

#[derive(axum_helpers::serde::Serialize, axum_helpers::serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct Gadget {
    id: i64,
    owner_id: i64,
}

/// The filter the `*Where` routes extract from the URL path. `Into<GadgetFilter>` for
/// itself comes from the blanket `From<T> for T`, so it can serve as its own `PathParams`.
#[derive(axum_helpers::serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct GadgetFilter {
    owner_id: i64,
}

// --- SQL trait impls. Bodies only need to type-check; nothing here touches a database. ---

impl HasPrimaryKey for Gadget {
    type PrimaryKey = i64;
}

#[async_trait]
impl GetLatestRecord for Gadget {
    async fn get_latest_record(_pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl GetRecord for Gadget {
    async fn get_record(
        _pool: &PgPool,
        _primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl GetRecordWhere<GadgetFilter> for Gadget {
    async fn get_record_where(
        _pool: &PgPool,
        _where_params: GadgetFilter,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl ListRecords for Gadget {
    async fn get_all(_pool: &PgPool) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ListRecordsWhere<GadgetFilter> for Gadget {
    async fn get_records(
        _pool: &PgPool,
        _where_params: GadgetFilter,
    ) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl InsertSQL for Gadget {
    type ReturnType = Gadget;
    async fn insert_sql(self, _pool: &PgPool) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(self)
    }
}

#[async_trait]
impl BulkInsertSQL for Gadget {
    type ReturnType = u64;
    async fn bulk_insert_sql(
        _pool: &PgPool,
        records: &[Self],
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(records.len() as u64)
    }
}

#[async_trait]
impl DeleteSQL for Gadget {
    type ReturnType = ();
    async fn delete_sql(
        _pool: &PgPool,
        _primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

// A non-`()` `ReturnType` is the point of `DeleteRecordsWhere`: unlike `DeleteSQL`, whose
// route throws the value away and answers `204`, this one serializes it into a `200` body.
#[async_trait]
impl DeleteRecordsWhere<GadgetFilter> for Gadget {
    type ReturnType = u64;
    async fn delete_records_where(
        _pool: &PgPool,
        _where_params: GadgetFilter,
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(0)
    }
}

// --- Route trait impls. ---

impl GetLatestRoute for Gadget {}
impl GetRecordRoute for Gadget {}
impl ListRecordsRoute for Gadget {}
impl DeleteRoute for Gadget {}
impl<'de> CreateRoute<'de> for Gadget {}
impl<'de> BulkCreateRoute<'de> for Gadget {}

impl GetRecordWhereRoute<GadgetFilter> for Gadget {
    type PathParams = GadgetFilter;
}

impl ListRecordsWhereRoute<GadgetFilter> for Gadget {
    type PathParams = GadgetFilter;
}

impl DeleteRecordsWhereRoute<GadgetFilter> for Gadget {
    type PathParams = GadgetFilter;
}

/// Every handler mounted on one router. If any of them stops satisfying axum's `Handler`
/// trait this stops compiling, which is the whole assertion.
fn build_router() -> Router<PgPool> {
    Router::<PgPool>::new()
        .route("/gadgets/latest", get(Gadget::get_latest_route))
        .route("/gadgets/{id}", get(Gadget::get_record_route))
        .route("/gadgets", get(Gadget::list_records_route))
        .route("/gadgets/{id}", delete(Gadget::delete_route))
        .route("/gadgets", post(Gadget::create_route))
        .route("/gadgets/bulk", post(Gadget::bulk_create_route))
        .route(
            "/owners/{owner_id}/gadget",
            get(<Gadget as GetRecordWhereRoute<GadgetFilter>>::get_record_where_route),
        )
        .route(
            "/owners/{owner_id}/gadgets",
            get(<Gadget as ListRecordsWhereRoute<GadgetFilter>>::list_records_where_route),
        )
        .route(
            "/owners/{owner_id}/gadgets",
            delete(<Gadget as DeleteRecordsWhereRoute<GadgetFilter>>::delete_records_where_route),
        )
}

#[test]
fn every_route_handler_mounts_on_a_router() {
    let _router = build_router();
}
