use async_trait::async_trait;
use axum::{
    extract::{self, Path, Query, State, rejection::QueryRejection},
    http::{StatusCode, header},
    response::{self, IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use sqlx::Pool;

use sql_traits::{
    BulkInsertRecords, DeleteRecord, DeleteRecordsWhere, GetLatestRecord, GetRecord,
    GetRecordWhere, HasDatabase, HasPrimaryKey, HasRequestBody, HasUpdateFields, InsertRecord,
    ListRecords, ListRecordsPaginated, ListRecordsWhere, ListRecordsWherePaginated,
    PaginationParams, ReplaceRecord, UpdateFields, UpdateRecord,
};

use crate::{ApiError, ApiErrorResponse, PaginationQuery, PathAndQuery};

/// Renders the `Result<Option<Record>, ApiError>` shape shared by every fetch-one route
/// handler: `200 OK` with the record as JSON, `on_missing()` when the query matched no row,
/// or the error's own response. The handlers differ only in what "no row" means, so that is
/// the sole parameter.
fn optional_record_response<T: serde::Serialize>(
    result: Result<Option<T>, ApiError>,
    on_missing: fn() -> Response,
) -> Response {
    match result {
        Ok(Some(record)) => (StatusCode::OK, response::Json(record)).into_response(),
        Ok(None) => on_missing(),
        Err(api_error) => api_error.into_response(),
    }
}

/// Renders a mutation's successful `ReturnType`, the default behind every `*_response` hook:
/// `with_body` and the value as JSON, or `without_body` with no body at all when the value
/// serializes to JSON `null` — which `()`, a unit struct and `None` all do.
///
/// Deciding on the serialized bytes rather than the type is what lets this serve every
/// `Serialize` return type without a trait of its own: stable Rust cannot specialize on `()`,
/// and the value has to be serialized for the response anyway, so the check costs one
/// four-byte comparison. A value that fails to serialize is `ApiError::Serde`, a `500` in this
/// crate's `{"message": …}` shape, rather than axum's plain-text one.
fn mutation_response<T: serde::Serialize>(
    value: T,
    with_body: StatusCode,
    without_body: StatusCode,
) -> Response {
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes == b"null" => without_body.into_response(),
        Ok(bytes) => (
            with_body,
            [(header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        Err(error) => ApiError::Serde(error).into_response(),
    }
}

/// `204 No Content`: the query is well-formed and an empty result is not an error.
fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// `404 Not Found`: the caller addressed a specific record and it does not exist.
fn not_found() -> Response {
    ApiError::NotFoundError.into_response()
}

/// Unwraps a paginated route's query extractor: axum's own rejection and an out-of-range limit
/// both become this crate's `{"message": …}` body, in one place for every implementor.
///
/// A bare `Query<Q>` would render its rejection as axum's plain-text default, which would make an
/// unparseable `?limit=abc` the one error here whose body is not a message object.
///
/// The `Err` is an [`ApiError`] rather than a rendered `Response` because both failures are
/// already variants of it: the rejection widens through `ApiError::InvalidQueryParams` and the
/// limit check through `ValidationError`, so each arrives with `?` and the rendering stays in the
/// one `From<ApiError>` impl. It is also what keeps `clippy::result_large_err` quiet — a
/// `Response` is 128 bytes, the lint's own threshold, and an `ApiError` is 48.
fn validated_query<Q: PaginationQuery>(
    query: Result<Query<Q>, QueryRejection>,
) -> Result<Q, ApiError> {
    let Query(params) = query?;
    params.validate()?;
    Ok(params)
}

/// Axum route handler for fetching the most recent record.
///
/// Returns `200 OK` with the record as JSON, or `204 No Content` if none exists.
#[async_trait]
pub trait GetLatestRoute: GetLatestRecord + serde::Serialize {
    // TODO: Add a function for logging
    async fn get_latest_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
    ) -> Response {
        let result = Self::get_latest_record(&pool).await.map_err(ApiError::from);
        optional_record_response(result, no_content)
    }
}

/// Axum route handler for getting a record by its primary key extracted from the URL path.
///
/// Returns `200 OK` with the record as JSON, or `404 Not Found` if no such record exists.
///
/// # Composite primary keys
///
/// A composite `PrimaryKey` is a generated `{Name}PrimaryKey` struct, so axum binds path
/// segments **by name**: each segment must be named after the field it fills, and the order
/// the route declares them in does not matter. A segment naming no field is ignored, so a
/// route may capture more than the key — `/orgs/{org_id}/memberships/{user_id}/{group_id}`
/// works — and a key field no segment names is a `400`, answered by the extractor before
/// this handler runs.
#[async_trait]
pub trait GetRecordRoute: GetRecord + serde::Serialize
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
{
    // TODO: Add a function for logging
    async fn get_record_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
    ) -> Response {
        let result = Self::get_record(&pool, primary_key)
            .await
            .map_err(ApiError::from);
        optional_record_response(result, not_found)
    }
}

/// Axum route handler for fetching a single record matching a filter taken from the URL path
/// and query string.
///
/// Returns `200 OK` with the record as JSON, `404 Not Found` if no record matches, or
/// `400 Bad Request` if the filter cannot be filled.
///
/// # Where the filter comes from
///
/// `WhereParams` is read through [`PathAndQuery`]: the route's path segments and the query
/// string merged into one set of keys, so the route that mounts this handler decides which
/// source supplies each field. A key in both is a `400`. `WhereParams` must be a struct with
/// named fields; [`PathAndQuery`] documents the rest of what it can be.
#[async_trait]
pub trait GetRecordWhereRoute<T: Send>: GetRecordWhere<T> + serde::Serialize {
    type WhereParams: Into<T> + DeserializeOwned + Send + 'static;
    // TODO: Add a function for logging
    async fn get_record_where_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        PathAndQuery(where_params): PathAndQuery<Self::WhereParams>,
    ) -> Response {
        let result = Self::get_record_where(&pool, where_params.into())
            .await
            .map_err(ApiError::from);
        optional_record_response(result, not_found)
    }
}

/// Axum route handler for listing all records.
///
/// Returns `200 OK` with the records as a JSON array.
#[async_trait]
pub trait ListRecordsRoute: ListRecords + serde::Serialize {
    // TODO: Add a function for logging
    async fn list_records_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
    ) -> Response {
        Self::list_records(&pool)
            .await
            .map_err(ApiError::from)
            .map(|records| (StatusCode::OK, response::Json(records)))
            .into_response()
    }
}

/// Axum route handler for listing one page of records.
///
/// Returns `200 OK` with `{"data": [...], "pagination": {...}}`, or `400 Bad Request` if the
/// query string is unparseable or asks for a limit outside the range `Q` allows.
///
/// The pagination mode is `Q`: `OffsetParamsQuery<..>` or `CursorParamsQuery<C, ..>`, each
/// carrying its own default and maximum limit as const parameters. Neither takes a parameter
/// default, so both numbers are always written; the crate's own policy has a name. A mount site
/// names the mode, which is also where the policy is visible:
///
/// ```ignore
/// impl ListRecordsPaginatedRoute<DefaultOffsetParamsQuery> for Widget {}     // 50 / 200
/// impl ListRecordsPaginatedRoute<OffsetParamsQuery<20, 100>> for Invoice {}  // tuned
/// ```
///
/// # Always an envelope, never a bare array
///
/// [`ListRecordsRoute`] answers a JSON array and is unaffected by this trait. A type opts into
/// pagination by naming this one instead, and then every response is an envelope — including an
/// empty page, which is `200` with an empty `data`, never a `204`. Nothing branches on whether
/// the request carried pagination parameters.
///
/// # Why the query extractor is a `Result`
///
/// A bare `Query<Q>` rejection renders as axum's default plain-text body, which would make an
/// unparseable `?limit=abc` the one error in this crate that is not `{"message": "..."}`.
/// Extracting `Result<Query<Q>, QueryRejection>` moves that rendering here, once, for every
/// implementor.
///
/// # Both modes on one type
///
/// A type may implement this trait for an offset query type *and* a cursor one. Both impls then
/// carry the same provided-method name, so a mount site disambiguates:
/// `<Widget as ListRecordsPaginatedRoute<DefaultOffsetParamsQuery>>::list_records_paginated_route`.
/// That is the existing situation for the `*Where` family, not a new one.
#[async_trait]
pub trait ListRecordsPaginatedRoute<Q>: ListRecordsPaginated<Q::Params> + serde::Serialize
where
    Q: PaginationQuery,
    <Q::Params as PaginationParams>::Pagination: serde::Serialize,
{
    // TODO: Add a function for logging
    async fn list_records_paginated_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        query: Result<Query<Q>, QueryRejection>,
    ) -> Response {
        let params = match validated_query(query) {
            Ok(params) => params,
            Err(error) => return error.into_response(),
        };

        Self::list_records_paginated(&pool, params.into())
            .await
            .map_err(ApiError::from)
            .map(|page| (StatusCode::OK, response::Json(page)))
            .into_response()
    }
}

/// Axum route handler for listing records matching a filter taken from the URL path and query
/// string.
///
/// Returns `200 OK` with the matching records as a JSON array, or `400 Bad Request` if the
/// filter cannot be filled.
///
/// # Where the filter comes from
///
/// `WhereParams` is read through [`PathAndQuery`]: the route's path segments and the query
/// string merged into one set of keys, so the route that mounts this handler decides which
/// source supplies each field. A key in both is a `400`. `WhereParams` must be a struct with
/// named fields; [`PathAndQuery`] documents the rest of what it can be.
#[async_trait]
pub trait ListRecordsWhereRoute<T: Send>: ListRecordsWhere<T> + serde::Serialize {
    type WhereParams: Into<T> + DeserializeOwned + Send + 'static;
    // TODO: Add a function for logging
    async fn list_records_where_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        PathAndQuery(where_params): PathAndQuery<Self::WhereParams>,
    ) -> Response {
        Self::list_records_where(&pool, where_params.into())
            .await
            .map_err(ApiError::from)
            .map(|records| (StatusCode::OK, response::Json(records)))
            .into_response()
    }
}

/// Axum route handler for listing one page of the records matching a filter taken from the URL
/// path and query string.
///
/// Returns `200 OK` with `{"data": [...], "pagination": {...}}`, or `400 Bad Request` if the
/// filter cannot be filled, or the query string is unparseable or asks for a limit outside the
/// range `Q` allows.
///
/// Everything in [`ListRecordsPaginatedRoute`]'s documentation applies: the envelope is
/// unconditional, the pagination mode is `Q`, the query extractor is a `Result` so a rejection
/// is rendered in this crate's error shape, and a type may implement the trait once per mode.
///
/// The filter and the pagination parameters share the query string, and each ignores the
/// other's keys: `?genre=SciFi&limit=5` fills `WhereParams` with `genre` and `Q` with `limit`.
/// That only holds while `WhereParams` tolerates unknown fields, so a filter carrying
/// `#[serde(deny_unknown_fields)]` rejects every paginated request.
///
/// Like the rest of the `*Where` family, `WhereParams` has to be chosen by the implementor —
/// it cannot be inferred from the record — which is why no derive can emit this impl.
///
/// # Where the filter comes from
///
/// `WhereParams` is read through [`PathAndQuery`]: the route's path segments and the query
/// string merged into one set of keys, so the route that mounts this handler decides which
/// source supplies each field. A key in both is a `400`. `WhereParams` must be a struct with
/// named fields; [`PathAndQuery`] documents the rest of what it can be.
#[async_trait]
pub trait ListRecordsWherePaginatedRoute<T: Send, Q>:
    ListRecordsWherePaginated<T, Q::Params> + serde::Serialize
where
    Q: PaginationQuery,
    <Q::Params as PaginationParams>::Pagination: serde::Serialize,
{
    type WhereParams: Into<T> + DeserializeOwned + Send + 'static;

    // TODO: Add a function for logging
    async fn list_records_where_paginated_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        PathAndQuery(where_params): PathAndQuery<Self::WhereParams>,
        query: Result<Query<Q>, QueryRejection>,
    ) -> Response {
        let params = match validated_query(query) {
            Ok(params) => params,
            Err(error) => return error.into_response(),
        };

        Self::list_records_where_paginated(&pool, where_params.into(), params.into())
            .await
            .map_err(ApiError::from)
            .map(|page| (StatusCode::OK, response::Json(page)))
            .into_response()
    }
}

/// Axum route handler for creating a single record from a JSON request body.
///
/// Returns `201 Created` with the `InsertRecord::ReturnType` as JSON, or `201 Created` with no
/// body when the `ReturnType` serializes to `null` (`()`, a unit struct, `None`). Override
/// [`create_response`](CreateRoute::create_response) to answer differently.
#[async_trait]
pub trait CreateRoute<'de>: InsertRecord + serde::Deserialize<'de>
where
    <Self as InsertRecord>::ReturnType: serde::Serialize,
{
    /// Renders a successful insert. Only ever called with a success: a failed insert is
    /// rendered as this crate's error response without reaching this method, so an override
    /// cannot change the error shape.
    ///
    /// The default answers `201 Created` with `created` as JSON, or with no body when it
    /// serializes to `null`.
    fn create_response(created: <Self as InsertRecord>::ReturnType) -> Response {
        mutation_response(created, StatusCode::CREATED, StatusCode::CREATED)
    }

    // TODO: Add a function for logging
    async fn create_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        extract::Json(obj): extract::Json<Self>,
    ) -> Response {
        match obj.insert_record(&pool).await {
            Ok(created) => Self::create_response(created),
            Err(error) => ApiError::from(error).into_response(),
        }
    }
}

/// Axum route handler for creating multiple records from a JSON array request body.
///
/// Returns `201 Created` with the `BulkInsertRecords::ReturnType` as JSON, or `201 Created`
/// with no body when the `ReturnType` serializes to `null` — so a bulk insert that returns
/// `()` to avoid fetching every created row answers an empty `201`, not a `null` body.
/// Override [`bulk_create_response`](BulkCreateRoute::bulk_create_response) to answer
/// differently.
#[async_trait]
pub trait BulkCreateRoute<'de>: BulkInsertRecords + serde::Deserialize<'de>
where
    <Self as BulkInsertRecords>::ReturnType: serde::Serialize,
{
    /// Renders a successful bulk insert. Only ever called with a success, like
    /// [`CreateRoute::create_response`].
    ///
    /// The default answers `201 Created` with `created` as JSON, or with no body when it
    /// serializes to `null`.
    fn bulk_create_response(created: <Self as BulkInsertRecords>::ReturnType) -> Response {
        mutation_response(created, StatusCode::CREATED, StatusCode::CREATED)
    }

    // TODO: Add a function for logging
    async fn bulk_create_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        extract::Json(objs): extract::Json<Vec<Self>>,
    ) -> Response {
        match Self::bulk_insert_records(&pool, &objs).await {
            Ok(created) => Self::bulk_create_response(created),
            Err(error) => ApiError::from(error).into_response(),
        }
    }
}

/// Axum route handler for deleting a record by its primary key extracted from the URL path.
///
/// Returns `200 OK` with the `DeleteRecord::ReturnType` as JSON, or `204 No Content` when the
/// `ReturnType` serializes to `null` (`()`, a unit struct, `None`). A delete returning a row
/// count therefore answers `200` with the count; one that should answer `204` regardless
/// either returns `()` or overrides [`delete_response`](DeleteRoute::delete_response).
///
/// `None` is not read as "no such row": it is a `204` like `()`, never a `404`. A delete
/// that wants to report a missing key does so by overriding the hook.
///
/// # Composite primary keys
///
/// A composite `PrimaryKey` is a generated `{Name}PrimaryKey` struct, so axum binds path
/// segments **by name**: each segment must be named after the field it fills, and the order
/// the route declares them in does not matter. A segment naming no field is ignored, so a
/// route may capture more than the key — `/orgs/{org_id}/memberships/{user_id}/{group_id}`
/// works — and a key field no segment names is a `400`, answered by the extractor before
/// this handler runs.
#[async_trait]
pub trait DeleteRoute: DeleteRecord + HasPrimaryKey
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
    <Self as DeleteRecord>::ReturnType: serde::Serialize,
{
    /// Renders a successful delete. Only ever called with a success: a failed delete is
    /// rendered as this crate's error response without reaching this method, so an override
    /// cannot change the error shape.
    ///
    /// The default answers `200 OK` with `deleted` as JSON, or `204 No Content` when it
    /// serializes to `null`. An override still needs the `ReturnType: Serialize` bound the
    /// trait declares, since the default has to compile for every implementor.
    fn delete_response(deleted: <Self as DeleteRecord>::ReturnType) -> Response {
        mutation_response(deleted, StatusCode::OK, StatusCode::NO_CONTENT)
    }

    // TODO: Add a function for logging
    async fn delete_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
    ) -> Response {
        match <Self as DeleteRecord>::delete_record(&pool, primary_key).await {
            Ok(deleted) => Self::delete_response(deleted),
            Err(error) => ApiError::from(error).into_response(),
        }
    }
}

/// Axum route handler for deleting records matching a filter taken from the URL path and query
/// string.
///
/// Returns `200 OK` with `DeleteRecordsWhere<T>::ReturnType` as JSON, so a caller can see what
/// the delete matched, or `204 No Content` when the `ReturnType` serializes to `null`.
/// Override [`delete_records_where_response`](DeleteRecordsWhereRoute::delete_records_where_response)
/// to answer differently. A filter that cannot be filled is a `400` before anything is deleted.
///
/// # Where the filter comes from
///
/// `WhereParams` is read through [`PathAndQuery`]: the route's path segments and the query
/// string merged into one set of keys, so the route that mounts this handler decides which
/// source supplies each field. A key in both is a `400`. `WhereParams` must be a struct with
/// named fields; [`PathAndQuery`] documents the rest of what it can be.
#[async_trait]
pub trait DeleteRecordsWhereRoute<T: Send>: DeleteRecordsWhere<T>
where
    <Self as DeleteRecordsWhere<T>>::ReturnType: serde::Serialize,
{
    type WhereParams: Into<T> + DeserializeOwned + Send + 'static;

    /// Renders a successful filtered delete. Only ever called with a success, like
    /// [`DeleteRoute::delete_response`].
    ///
    /// The default answers `200 OK` with `deleted` as JSON, or `204 No Content` when it
    /// serializes to `null`.
    fn delete_records_where_response(
        deleted: <Self as DeleteRecordsWhere<T>>::ReturnType,
    ) -> Response {
        mutation_response(deleted, StatusCode::OK, StatusCode::NO_CONTENT)
    }

    // TODO: Add a function for logging
    async fn delete_records_where_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        PathAndQuery(where_params): PathAndQuery<Self::WhereParams>,
    ) -> Response {
        match Self::delete_records_where(&pool, where_params.into()).await {
            Ok(deleted) => Self::delete_records_where_response(deleted),
            Err(error) => ApiError::from(error).into_response(),
        }
    }
}

/// Axum route handler for replacing an entire record. The primary key comes from the URL
/// path; the JSON body is `HasRequestBody::RequestBody`, which carries every field
/// *except* the key.
///
/// Returns `200 OK` with the replaced record as JSON, or `404 Not Found` if the key
/// matches no row — the same convention `GetRecordRoute` follows.
///
/// # Why the body has no primary key
///
/// The path is the only place the key is read from, so there is no second key to
/// reconcile with it. By default a client that sends the key in the body anyway still
/// succeeds — serde ignores unknown fields — and the value is discarded. A generated
/// OpenAPI schema therefore describes the body without the key, without needing to be
/// told to hide it.
///
/// That tolerance is serde's default, not a promise this trait makes. [`sql_traits::Record`]
/// forwards every non-`sql_traits` container attribute to the generated body, so a record
/// carrying `#[serde(deny_unknown_fields)]` yields a body that carries it too, and a
/// payload with the key in it is then rejected with `422 Unprocessable Entity`. That is
/// deliberate: an explicit opt-in to strictness is honoured rather than quietly
/// overridden. A type that needs the tolerance should not declare that attribute.
///
/// # Composite primary keys
///
/// A composite `PrimaryKey` is a generated `{Name}PrimaryKey` struct, so axum binds path
/// segments **by name**: each segment must be named after the field it fills, and the order
/// the route declares them in does not matter. A segment naming no field is ignored, so a
/// route may capture more than the key — `/orgs/{org_id}/memberships/{user_id}/{group_id}`
/// works — and a key field no segment names is a `400`, answered by the extractor before
/// this handler runs.
#[async_trait]
pub trait ReplaceRoute: ReplaceRecord + HasRequestBody + serde::Serialize
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
    <Self as HasRequestBody>::RequestBody: DeserializeOwned + Send + 'static,
{
    // TODO: Add a function for logging
    async fn replace_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
        extract::Json(body): extract::Json<<Self as HasRequestBody>::RequestBody>,
    ) -> Response {
        let result = <Self as HasRequestBody>::from_request_body(body, primary_key)
            .replace_record(&pool)
            .await
            .map_err(ApiError::from);
        optional_record_response(result, not_found)
    }
}

/// Axum route handler for a partial update. The primary key comes from the URL path; the
/// JSON body is `HasUpdateFields::UpdateFields`, which carries every non-key field
/// optionally, so a caller names only what it means to change.
///
/// Returns `200 OK` with the updated record as JSON, `400 Bad Request` if the body sets no
/// field at all, or `404 Not Found` if the key matches no row.
///
/// # Why an empty body is a `400`
///
/// An update whose `SET` list is built from the fields that are present has no statement to
/// run when none of them are. Left to the implementation that is a SQL syntax error and a
/// `500`; checked here it is one branch, before the pool is touched, for every implementor
/// at once. The check runs before the key is looked up, so an empty body is a `400` whether
/// or not the row exists.
///
/// # Clearing a nullable column
///
/// A field that is absent from the body is left alone; a nullable field explicitly set to
/// `null` is cleared. Keeping those apart is the whole reason [`sql_traits::Update`] emits
/// `sql_traits::double_option` on nullable fields — a hand-written update type that omits
/// it will silently treat `null` as "leave alone".
///
/// # Composite primary keys
///
/// A composite `PrimaryKey` is a generated `{Name}PrimaryKey` struct, so axum binds path
/// segments **by name**: each segment must be named after the field it fills, and the order
/// the route declares them in does not matter. A segment naming no field is ignored, so a
/// route may capture more than the key — `/orgs/{org_id}/memberships/{user_id}/{group_id}`
/// works — and a key field no segment names is a `400`, answered by the extractor before
/// this handler runs.
#[async_trait]
pub trait UpdateRoute: UpdateRecord + serde::Serialize
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
    <Self as HasUpdateFields>::UpdateFields: DeserializeOwned + Send + 'static,
{
    // TODO: Add a function for logging
    async fn update_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
        extract::Json(update_fields): extract::Json<<Self as HasUpdateFields>::UpdateFields>,
    ) -> Response {
        if update_fields.is_empty() {
            return ApiErrorResponse::BadRequestWithMessage("Empty request body".to_string())
                .into_response();
        }

        let result = Self::update_record(&pool, primary_key, update_fields)
            .await
            .map_err(ApiError::from);
        optional_record_response(result, not_found)
    }
}
