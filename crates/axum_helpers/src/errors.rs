use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use error_set::error_set;

/// A convenience `Result` type that uses [`ApiError`] as the error variant.
pub type Result<T> = std::result::Result<T, ApiError>;

error_set! {
    ApiError := {
        NotFoundError,
    } || IOError
    IOError := {
        Serde(serde_json::Error),
        Sql(sqlx::Error),
    }
}

impl IntoResponse for ApiError {
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
            // TODO: Figure out a better way to differentiate serialization vs deserialization, as
            // a deserialization error should throw a 400 and a serialization error should throw a
            // 500
            ApiError::Serde(_) => ApiErrorResponse::BadRequestWithMessage(value.to_string()),
            ApiError::Sql(_) => ApiErrorResponse::InternalServerErrorWithMessage(value.to_string()),
        }
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
