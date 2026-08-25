use async_trait::async_trait;
use axum::{
    extract::{self, Path, State},
    http::StatusCode,
    response::{self, IntoResponse, Response},
};
use serde::de::DeserializeOwned;
use sqlx::PgPool;

use sql_traits::{
    BulkInsertSQL, DeleteSQL, GetLatestRecord, GetRecord, GetRecordWhere, HasPrimaryKey, InsertSQL,
    ListRecords, ListRecordsWhere,
};

use crate::ApiError;

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

/// `204 No Content`: the query is well-formed and an empty result is not an error.
fn no_content() -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// `404 Not Found`: the caller addressed a specific record and it does not exist.
fn not_found() -> Response {
    ApiError::NotFoundError.into_response()
}

/// Axum route handler for fetching the most recent record.
///
/// Returns `200 OK` with the record as JSON, or `204 No Content` if none exists.
#[async_trait]
pub trait GetLatestRoute: GetLatestRecord + serde::Serialize {
    // TODO: Add a function for logging
    async fn get_latest_route(State(pool): State<PgPool>) -> Response {
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
/// A composite `PrimaryKey` is a tuple, so axum binds path segments **positionally, not by
/// name**: the route's segment order must match the order of the `#[macros(primary_key)]`
/// fields on the struct. Declaring them out of order compiles and runs, but queries the
/// wrong row.
#[async_trait]
pub trait GetRecordRoute: GetRecord + serde::Serialize
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
{
    // TODO: Add a function for logging
    async fn get_record_route(
        State(pool): State<PgPool>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
    ) -> Response {
        let result = Self::get_record(&pool, primary_key)
            .await
            .map_err(ApiError::from);
        optional_record_response(result, not_found)
    }
}

/// Axum route handler for fetching a single record matching a filter extracted from the URL path.
///
/// Returns `200 OK` with the record as JSON, or `404 Not Found` if no record matches.
#[async_trait]
pub trait GetRecordWhereRoute<T: Send>: GetRecordWhere<T> + serde::Serialize {
    type PathParams: Into<T> + DeserializeOwned + Send + 'static;
    // TODO: Add a function for logging
    async fn get_record_where_route(
        State(pool): State<PgPool>,
        Path(path_params): Path<Self::PathParams>,
    ) -> Response {
        let result = Self::get_record_where(&pool, path_params.into())
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
    async fn list_records_route(State(pool): State<PgPool>) -> Response {
        Self::get_all(&pool)
            .await
            .map_err(ApiError::from)
            .map(|records| (StatusCode::OK, response::Json(records)))
            .into_response()
    }
}

/// Axum route handler for listing records matching a filter extracted from the URL path.
///
/// Returns `200 OK` with the matching records as a JSON array.
#[async_trait]
pub trait ListRecordsWhereRoute<T: Send>: ListRecordsWhere<T> + serde::Serialize {
    type PathParams: Into<T> + DeserializeOwned + Send + 'static;
    async fn list_records_where_route(
        State(pool): State<PgPool>,
        Path(path_params): Path<Self::PathParams>,
    ) -> Response {
        Self::get_records(&pool, path_params.into())
            .await
            .map_err(ApiError::from)
            .map(|records| (StatusCode::OK, response::Json(records)))
            .into_response()
    }
}

/// Axum route handler for creating a single record from a JSON request body.
///
/// Returns `201 Created` with the `InsertSQL::ReturnType` as JSON.
#[async_trait]
pub trait CreateRoute<'de>: InsertSQL + serde::Deserialize<'de>
where
    <Self as InsertSQL>::ReturnType: serde::Serialize,
{
    // TODO: Add a function for logging
    async fn create_route(
        State(pool): State<PgPool>,
        extract::Json(obj): extract::Json<Self>,
    ) -> Response {
        obj.insert_sql(&pool)
            .await
            .map_err(ApiError::from)
            .map(|record| (StatusCode::CREATED, response::Json(record)))
            .into_response()
    }
}

/// Axum route handler for creating multiple records from a JSON array request body.
///
/// Returns `201 Created` with the `BulkInsertSQL::ReturnType` as JSON.
#[async_trait]
pub trait BulkCreateRoute<'de>: BulkInsertSQL + serde::Deserialize<'de>
where
    <Self as BulkInsertSQL>::ReturnType: serde::Serialize,
{
    // TODO: Add a function for logging
    async fn bulk_create_route(
        State(pool): State<PgPool>,
        extract::Json(objs): extract::Json<Vec<Self>>,
    ) -> Response {
        Self::bulk_insert_sql(&pool, &objs)
            .await
            .map_err(ApiError::from)
            .map(|records| (StatusCode::CREATED, response::Json(records)))
            .into_response()
    }
}

/// Axum route handler for deleting a record by its primary key extracted from the URL path.
///
/// # Composite primary keys
///
/// A composite `PrimaryKey` is a tuple, so axum binds path segments **positionally, not by
/// name**: the route's segment order must match the order of the `#[macros(primary_key)]`
/// fields on the struct. Declaring them out of order compiles and runs, but deletes the
/// wrong row.
#[async_trait]
pub trait DeleteRoute: DeleteSQL + HasPrimaryKey
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
{
    // TODO: Add a function for logging
    async fn delete_route(
        State(pool): State<PgPool>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
    ) -> Response {
        <Self as DeleteSQL>::delete_sql(&pool, primary_key)
            .await
            .map_err(ApiError::from)
            .map(|_| StatusCode::NO_CONTENT)
            .into_response()
    }
}
