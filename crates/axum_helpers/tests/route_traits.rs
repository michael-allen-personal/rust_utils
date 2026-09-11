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
    routing::{delete, get, patch, post, put},
};
use axum_helpers::sql_traits::{
    BulkInsertRecords, DeleteRecord, DeleteRecordsWhere, GetLatestRecord, GetRecord,
    GetRecordWhere, HasPrimaryKey, HasRequestBody, HasUpdateFields, InsertRecord, ListRecords,
    ListRecordsWhere, ReplaceRecord, UpdateFields, UpdateRecord,
};
use axum_helpers::sqlx::{self, PgPool};
use axum_helpers::{
    BulkCreateRoute, CreateRoute, DeleteRecordsWhereRoute, DeleteRoute, GetLatestRoute,
    GetRecordRoute, GetRecordWhereRoute, ListRecordsPaginatedRoute, ListRecordsRoute,
    ListRecordsWherePaginatedRoute, ListRecordsWhereRoute, ReplaceRoute, UpdateRoute,
};

/// The primary key the fixtures treat as matching no row.
const MISSING_ID: i64 = 404;

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

    fn primary_key(&self) -> <Self as HasPrimaryKey>::PrimaryKey {
        self.id
    }
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
    async fn list_records(_pool: &PgPool) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ListRecordsWhere<GadgetFilter> for Gadget {
    async fn list_records_where(
        _pool: &PgPool,
        _where_params: GadgetFilter,
    ) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl InsertRecord for Gadget {
    type ReturnType = Gadget;
    async fn insert_record(self, _pool: &PgPool) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(self)
    }
}

#[async_trait]
impl BulkInsertRecords for Gadget {
    type ReturnType = u64;
    async fn bulk_insert_records(
        _pool: &PgPool,
        records: &[Self],
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(records.len() as u64)
    }
}

#[async_trait]
impl DeleteRecord for Gadget {
    type ReturnType = ();
    async fn delete_record(
        _pool: &PgPool,
        _primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

// A non-`()` `ReturnType` is the point of `DeleteRecordsWhere`: unlike `DeleteRecord`, whose
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

/// The key-less body for `Gadget`. Written by hand rather than derived: this file's job is
/// to exercise the route traits against hand-written impls. `tests/derive_macros.rs`
/// covers the generated version.
#[derive(axum_helpers::serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct GadgetBody {
    owner_id: i64,
}

impl HasRequestBody for Gadget {
    type RequestBody = GadgetBody;
    fn from_request_body(
        body: GadgetBody,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Self {
        Gadget {
            id: primary_key,
            owner_id: body.owner_id,
        }
    }
}

// A key matching no row is `Ok(None)`, which the route turns into a `404`. Keying that
// off a sentinel id lets one fixture cover both the found and the missing case.
#[async_trait]
impl ReplaceRecord for Gadget {
    async fn replace_record(self, _pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        if self.id == MISSING_ID {
            return Ok(None);
        }
        Ok(Some(self))
    }
}

impl ReplaceRoute for Gadget {}

/// `Gadget`'s non-key fields, each optional. Hand-written like `GadgetBody` above;
/// `tests/derive_macros.rs` covers the generated version.
#[derive(axum_helpers::serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct GadgetUpdate {
    #[serde(default)]
    owner_id: Option<i64>,
}

impl HasUpdateFields for Gadget {
    type UpdateFields = GadgetUpdate;

    fn apply_update_fields(mut record: Self, fields: GadgetUpdate) -> Self {
        if let Some(owner_id) = fields.owner_id {
            record.owner_id = owner_id;
        }
        record
    }
}

impl UpdateFields for GadgetUpdate {
    type Record = Gadget;

    fn is_empty(&self) -> bool {
        self.owner_id.is_none()
    }
}

#[async_trait]
impl UpdateRecord for Gadget {
    async fn update_record(
        _pool: &PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
        update_fields: <Self as HasUpdateFields>::UpdateFields,
    ) -> Result<Option<Self>, sqlx::Error> {
        if primary_key == MISSING_ID {
            return Ok(None);
        }
        Ok(Some(update_fields.apply(Gadget {
            id: primary_key,
            owner_id: 0,
        })))
    }
}

impl UpdateRoute for Gadget {}

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

// --- The paginated list family ---------------------------------------------------------
//
// Two modes on one record type, which is the case that needs a turbofish at the mount site
// below: both impls carry the same provided method name.

#[async_trait]
impl axum_helpers::sql_traits::ListRecordsPaginated<axum_helpers::sql_traits::OffsetParams>
    for Gadget
{
    async fn list_records_paginated(
        _pool: &PgPool,
        params: axum_helpers::sql_traits::OffsetParams,
    ) -> Result<
        axum_helpers::sql_traits::Page<Self, axum_helpers::sql_traits::OffsetPagination>,
        sqlx::Error,
    > {
        Ok(axum_helpers::sql_traits::Page {
            data: Vec::new(),
            pagination: axum_helpers::sql_traits::OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: None,
            },
        })
    }
}

#[async_trait]
impl axum_helpers::sql_traits::ListRecordsPaginated<axum_helpers::sql_traits::CursorParams<i64>>
    for Gadget
{
    async fn list_records_paginated(
        _pool: &PgPool,
        params: axum_helpers::sql_traits::CursorParams<i64>,
    ) -> Result<
        axum_helpers::sql_traits::Page<Self, axum_helpers::sql_traits::CursorPagination<i64>>,
        sqlx::Error,
    > {
        Ok(axum_helpers::sql_traits::Page {
            data: Vec::new(),
            pagination: axum_helpers::sql_traits::CursorPagination {
                limit: params.limit,
                next: None,
            },
        })
    }
}

impl ListRecordsPaginatedRoute<axum_helpers::DefaultOffsetParamsQuery> for Gadget {}

impl ListRecordsPaginatedRoute<axum_helpers::DefaultCursorParamsQuery<i64>> for Gadget {}

#[async_trait]
impl
    axum_helpers::sql_traits::ListRecordsWherePaginated<
        GadgetFilter,
        axum_helpers::sql_traits::OffsetParams,
    > for Gadget
{
    async fn list_records_where_paginated(
        _pool: &PgPool,
        _where_params: GadgetFilter,
        params: axum_helpers::sql_traits::OffsetParams,
    ) -> Result<
        axum_helpers::sql_traits::Page<Self, axum_helpers::sql_traits::OffsetPagination>,
        sqlx::Error,
    > {
        Ok(axum_helpers::sql_traits::Page {
            data: Vec::new(),
            pagination: axum_helpers::sql_traits::OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: None,
            },
        })
    }
}

impl ListRecordsWherePaginatedRoute<GadgetFilter, axum_helpers::DefaultOffsetParamsQuery>
    for Gadget
{
    type PathParams = GadgetFilter;
}

// The fourth combination: the filtered trait in cursor mode. Both route traits in both modes are
// mounted below, because axum's `Handler` requirements are checked per monomorphization — a
// `Router::route` that accepts the offset instantiation of this trait says nothing about the
// cursor one.
#[async_trait]
impl
    axum_helpers::sql_traits::ListRecordsWherePaginated<
        GadgetFilter,
        axum_helpers::sql_traits::CursorParams<i64>,
    > for Gadget
{
    async fn list_records_where_paginated(
        _pool: &PgPool,
        _where_params: GadgetFilter,
        params: axum_helpers::sql_traits::CursorParams<i64>,
    ) -> Result<
        axum_helpers::sql_traits::Page<Self, axum_helpers::sql_traits::CursorPagination<i64>>,
        sqlx::Error,
    > {
        Ok(axum_helpers::sql_traits::Page {
            data: Vec::new(),
            pagination: axum_helpers::sql_traits::CursorPagination {
                limit: params.limit,
                next: None,
            },
        })
    }
}

impl ListRecordsWherePaginatedRoute<GadgetFilter, axum_helpers::DefaultCursorParamsQuery<i64>>
    for Gadget
{
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
        .route("/gadgets/{id}", put(Gadget::replace_route))
        .route("/gadgets/{id}", patch(Gadget::update_route))
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
        .route(
            "/gadgets/paged",
            get(<Gadget as ListRecordsPaginatedRoute<axum_helpers::DefaultOffsetParamsQuery>>::list_records_paginated_route),
        )
        .route(
            "/gadgets/streamed",
            get(<Gadget as ListRecordsPaginatedRoute<axum_helpers::DefaultCursorParamsQuery<i64>>>::list_records_paginated_route),
        )
        .route(
            "/owners/{owner_id}/gadgets/paged",
            get(<Gadget as ListRecordsWherePaginatedRoute<GadgetFilter, axum_helpers::DefaultOffsetParamsQuery>>::list_records_where_paginated_route),
        )
        .route(
            "/owners/{owner_id}/gadgets/streamed",
            get(<Gadget as ListRecordsWherePaginatedRoute<GadgetFilter, axum_helpers::DefaultCursorParamsQuery<i64>>>::list_records_where_paginated_route),
        )
}

#[test]
fn every_route_handler_mounts_on_a_router() {
    let _router = build_router();
}
