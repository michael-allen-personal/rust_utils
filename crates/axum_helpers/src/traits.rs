use async_trait::async_trait;
use axum::{
    extract::{self, Path, State},
    http::StatusCode,
    response::{self, IntoResponse, Response},
};
use sqlx::PgPool;

use sql_traits::{
    BulkInsertSQL, DeleteSQL, GetLatestRecord, HasPrimaryKey, InsertSQL, ListRecords,
};

use crate::ApiError;

/// Axum route handler for fetching the most recent record.
///
/// Returns `200 OK` with the record as JSON, or `204 No Content` if none exists.
#[async_trait]
pub trait GetLatestRoute: GetLatestRecord + serde::Serialize {
    // TODO: Add a function for logging
    async fn get_latest_route(State(pool): State<PgPool>) -> Response {
        let result = Self::get_latest_record(&pool).await.map_err(ApiError::from);
        match result {
            Ok(Some(record)) => (StatusCode::OK, response::Json(Some(record))).into_response(),
            Ok(None) => StatusCode::NO_CONTENT.into_response(),
            Err(api_error) => api_error.into_response(),
        }
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
            .map(|records| (StatusCode::OK, response::Json(records)).into_response())
            .into_response()
    }
}

/// Axum route handler for creating a single record from a JSON request body.
///
/// Returns `201 Created` with the created record as JSON.
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
            .map(|record| (StatusCode::CREATED, response::Json(record)).into_response())
            .into_response()
    }
}

/// Axum route handler for creating multiple records from a JSON array request body.
#[async_trait]
pub trait BulkCreateRoute<'de>: BulkInsertSQL + serde::Deserialize<'de> {
    // TODO: Add a function for logging
    async fn bulk_create_route(
        State(pool): State<PgPool>,
        extract::Json(objs): extract::Json<Vec<Self>>,
    ) -> Response {
        Self::bulk_insert_sql(&pool, &objs)
            .await
            .map_err(ApiError::from)
            .map(|_| StatusCode::NO_CONTENT)
            .into_response()
    }
}

/// Axum route handler for deleting a record by its primary key extracted from the URL path.
#[async_trait]
pub trait DeleteRoute: DeleteSQL + HasPrimaryKey {
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
