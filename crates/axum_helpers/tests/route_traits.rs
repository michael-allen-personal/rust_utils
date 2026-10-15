//! Compile test that every route trait's handler is actually mountable on an axum `Router`.
//!
//! Implementing a route trait and *using* it are separate checks. A type can satisfy a
//! route trait's bounds and still be rejected by `Router::route`, because that is where
//! axum's `Handler` requirements are enforced: `DeserializeOwned` on whatever `Path` or
//! `PathAndQuery` extracts, `IntoResponse` on the return, `Send + 'static` throughout.
//! `v0.7.0` tightened several traits precisely because a bad impl compiled and only failed at
//! the mount site with an opaque `Handler` error, so mounting is the check that matches the
//! failure mode.
//!
//! This also covers the `*Filtered` family (`GetRecordFilteredRoute`, `ListRecordsFilteredRoute`,
//! `DeleteRecordsFilteredRoute`), which the derive macros cannot emit — their `FilterParams`
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
    BulkInsertRecords, DeleteRecord, DeleteRecordsFiltered, GetLatestRecord, GetRecord,
    GetRecordFiltered, HasPrimaryKey, HasRequestBody, HasUpdateFields, InsertRecord, ListRecords,
    ListRecordsFiltered, ReplaceRecord, UpdateFields, UpdateRecord,
};
use axum_helpers::sqlx::{self, PgPool};
use axum_helpers::{
    BulkCreateRoute, CreateRoute, DeleteRecordsFilteredRoute, DeleteRoute, GetLatestRoute,
    GetRecordFilteredRoute, GetRecordRoute, ListRecordsFilteredPaginatedRoute,
    ListRecordsFilteredRoute, ListRecordsPaginatedRoute, ListRecordsRoute, ReplaceRoute,
    UpdateRoute,
};

/// The primary key the fixtures treat as matching no row.
const MISSING_ID: i64 = 404;

#[derive(
    axum_helpers::serde::Serialize, axum_helpers::serde::Deserialize, sql_traits::Database,
)]
#[sql_traits(database = Postgres)]
#[serde(crate = "axum_helpers::serde")]
struct Gadget {
    id: i64,
    owner_id: i64,
}

/// The filter the `*Filtered` routes extract from the URL path and query string.
/// `Into<GadgetFilter>` for itself comes from the blanket `From<T> for T`, so it can serve as
/// its own `FilterParams`.
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
impl GetRecordFiltered<GadgetFilter> for Gadget {
    async fn get_record_filtered(
        _pool: &PgPool,
        _filter_params: GadgetFilter,
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
impl ListRecordsFiltered<GadgetFilter> for Gadget {
    async fn list_records_filtered(
        _pool: &PgPool,
        _filter_params: GadgetFilter,
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

// A non-`()` `ReturnType` is the point of `DeleteRecordsFiltered`: unlike `DeleteRecord`, whose
// route throws the value away and answers `204`, this one serializes it into a `200` body.
#[async_trait]
impl DeleteRecordsFiltered<GadgetFilter> for Gadget {
    type ReturnType = u64;
    async fn delete_records_filtered(
        _pool: &PgPool,
        _filter_params: GadgetFilter,
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

impl GetRecordFilteredRoute<GadgetFilter> for Gadget {
    type FilterParams = GadgetFilter;
}

impl ListRecordsFilteredRoute<GadgetFilter> for Gadget {
    type FilterParams = GadgetFilter;
}

impl DeleteRecordsFilteredRoute<GadgetFilter> for Gadget {
    type FilterParams = GadgetFilter;
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
    axum_helpers::sql_traits::ListRecordsFilteredPaginated<
        GadgetFilter,
        axum_helpers::sql_traits::OffsetParams,
    > for Gadget
{
    async fn list_records_filtered_paginated(
        _pool: &PgPool,
        _filter_params: GadgetFilter,
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

impl ListRecordsFilteredPaginatedRoute<GadgetFilter, axum_helpers::DefaultOffsetParamsQuery>
    for Gadget
{
    type FilterParams = GadgetFilter;
}

// The fourth combination: the filtered trait in cursor mode. Both route traits in both modes are
// mounted below, because axum's `Handler` requirements are checked per monomorphization — a
// `Router::route` that accepts the offset instantiation of this trait says nothing about the
// cursor one.
#[async_trait]
impl
    axum_helpers::sql_traits::ListRecordsFilteredPaginated<
        GadgetFilter,
        axum_helpers::sql_traits::CursorParams<i64>,
    > for Gadget
{
    async fn list_records_filtered_paginated(
        _pool: &PgPool,
        _filter_params: GadgetFilter,
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

impl ListRecordsFilteredPaginatedRoute<GadgetFilter, axum_helpers::DefaultCursorParamsQuery<i64>>
    for Gadget
{
    type FilterParams = GadgetFilter;
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
            get(<Gadget as GetRecordFilteredRoute<GadgetFilter>>::get_record_filtered_route),
        )
        .route(
            "/owners/{owner_id}/gadgets",
            get(<Gadget as ListRecordsFilteredRoute<GadgetFilter>>::list_records_filtered_route),
        )
        .route(
            "/owners/{owner_id}/gadgets",
            delete(<Gadget as DeleteRecordsFilteredRoute<GadgetFilter>>::delete_records_filtered_route),
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
            get(<Gadget as ListRecordsFilteredPaginatedRoute<GadgetFilter, axum_helpers::DefaultOffsetParamsQuery>>::list_records_filtered_paginated_route),
        )
        .route(
            "/owners/{owner_id}/gadgets/streamed",
            get(<Gadget as ListRecordsFilteredPaginatedRoute<GadgetFilter, axum_helpers::DefaultCursorParamsQuery<i64>>>::list_records_filtered_paginated_route),
        )
}

#[test]
fn every_route_handler_mounts_on_a_router() {
    let _router = build_router();
}

// --- A second fixture on a second database ------------------------------------------
//
// `Gadget` above is Postgres. This one is SQLite, and the assertion is simply that both
// mount: the route traits carry no database of their own, they follow each record's
// `HasDatabase`. A regression that re-hard-coded one driver would fail to compile here.

use axum_helpers::sqlx::{Pool, Sqlite};

#[derive(axum_helpers::serde::Serialize, sql_traits::Database)]
#[sql_traits(database = Sqlite)]
#[serde(crate = "axum_helpers::serde")]
struct Sprocket {
    id: i64,
    name: String,
}

#[derive(axum_helpers::serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct SprocketFilter {
    name: String,
}

impl HasPrimaryKey for Sprocket {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl GetRecord for Sprocket {
    async fn get_record(
        _pool: &Pool<Sqlite>,
        _primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl ListRecords for Sprocket {
    async fn list_records(_pool: &Pool<Sqlite>) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ListRecordsFiltered<SprocketFilter> for Sprocket {
    async fn list_records_filtered(
        _pool: &Pool<Sqlite>,
        _filter_params: SprocketFilter,
    ) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

impl GetRecordRoute for Sprocket {}
impl ListRecordsRoute for Sprocket {}
impl ListRecordsFilteredRoute<SprocketFilter> for Sprocket {
    type FilterParams = SprocketFilter;
}

/// Mounting is the check that matches the failure mode: a handler can satisfy its trait
/// bounds and still be rejected by `Router::route`. No turbofish is needed — the state
/// type pins the database.
#[test]
fn sqlite_backed_handlers_mount_on_a_router() {
    let _router: Router<Pool<Sqlite>> = Router::new()
        .route("/sprockets/{id}", get(Sprocket::get_record_route))
        .route("/sprockets", get(Sprocket::list_records_route))
        .route(
            "/sprockets/by-name",
            get(Sprocket::list_records_filtered_route),
        );
}
