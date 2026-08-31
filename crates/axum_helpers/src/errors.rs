use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use error_set::error_set;

/// A convenience `Result` type whose error variant defaults to [`ApiError`] but can be
/// overridden, e.g. `Result<T, sqlx::Error>`.
pub type Result<T, E = ApiError> = std::result::Result<T, E>;

error_set! {
    ApiError := {
        NotFoundError,
        /// A query string axum's `Query` extractor could not deserialize — an unparseable
        /// `?limit=abc`, or a cursor type whose `Deserialize` rejected what it was handed.
        InvalidQueryParams(axum::extract::rejection::QueryRejection),
    } || IOError || ValidationError
    IOError := {
        Deserialize(serde_json::Error),
        Serialize(serde_json::Error),
        Sql(sqlx::Error),
    }
    /// What *validation* rejects about a request, as opposed to anything that went wrong
    /// serving it. A subset rather than inline variants so a validation function can
    /// return only what it can actually produce and widen into [`ApiError`] with `?`.
    ///
    /// `PartialEq` is derivable here and not on `ApiError`, whose `sqlx`, `serde_json` and
    /// `QueryRejection` sources are not comparable; it is what lets a validation result be
    /// asserted with `assert_eq!` rather than `matches!`. Keeping it derivable is the
    /// constraint on what may join this set.
    #[derive(PartialEq, Eq)]
    ValidationError := {
        #[display("`limit` must be between 1 and {max}, got {requested}")]
        InvalidPaginationLimit { requested: u16, max: u16 },
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        ApiErrorResponse::from(self).into_response()
    }
}

impl IntoResponse for IOError {
    fn into_response(self) -> Response {
        ApiErrorResponse::from(self).into_response()
    }
}

impl IntoResponse for ValidationError {
    fn into_response(self) -> Response {
        ApiErrorResponse::from(self).into_response()
    }
}

/// A structured HTTP error response with a status code and JSON message body.
#[derive(Debug)]
pub enum ApiErrorResponse {
    InternalServerError,
    InternalServerErrorWithMessage(String),
    NotFound,
    BadRequest,
    BadRequestWithMessage(String),
    FlexibleError(StatusCode, String),
}

impl From<ApiError> for ApiErrorResponse {
    fn from(value: ApiError) -> Self {
        match value {
            ApiError::NotFoundError => ApiErrorResponse::NotFound,
            ApiError::Deserialize(_)
            | ApiError::InvalidPaginationLimit { .. }
            | ApiError::InvalidQueryParams(_) => {
                ApiErrorResponse::BadRequestWithMessage(value.to_string())
            }
            ApiError::Serialize(_) | ApiError::Sql(_) => {
                ApiErrorResponse::InternalServerErrorWithMessage(value.to_string())
            }
        }
    }
}

impl From<IOError> for ApiErrorResponse {
    fn from(value: IOError) -> Self {
        ApiErrorResponse::from(ApiError::from(value))
    }
}

impl From<ValidationError> for ApiErrorResponse {
    fn from(value: ValidationError) -> Self {
        ApiErrorResponse::from(ApiError::from(value))
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        let (http_status, error_message) = match self {
            ApiErrorResponse::InternalServerError => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Internal Server Error".to_string(),
            ),
            ApiErrorResponse::InternalServerErrorWithMessage(error_message) => {
                (StatusCode::INTERNAL_SERVER_ERROR, error_message)
            }
            ApiErrorResponse::FlexibleError(status, error_message) => (status, error_message),
            ApiErrorResponse::NotFound => (StatusCode::NOT_FOUND, "Not Found".to_string()),
            ApiErrorResponse::BadRequest => (StatusCode::BAD_REQUEST, "Bad Request".to_string()),
            ApiErrorResponse::BadRequestWithMessage(error_message) => {
                (StatusCode::BAD_REQUEST, error_message)
            }
        };

        (
            http_status,
            Json(serde_json::json!({ "message": error_message })),
        )
            .into_response()
    }
}
