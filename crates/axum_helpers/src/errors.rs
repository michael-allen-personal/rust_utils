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
    } || IOError || RequestError
    IOError := {
        Serde(serde_json::Error),
        Sql(sqlx::Error),
    }
    /// Errors caused by what the client sent, as opposed to anything that went wrong
    /// serving it. A subset rather than inline variants so a validation function can
    /// return only what it can actually produce and widen into [`ApiError`] with `?`.
    ///
    /// `PartialEq` is derivable here and not on `ApiError`, whose `sqlx` and `serde_json`
    /// sources are not comparable; it is what lets a validation result be asserted with
    /// `assert_eq!` rather than `matches!`.
    #[derive(PartialEq, Eq)]
    RequestError := {
        #[display("`limit` must be between 1 and {max}, got {requested}")]
        InvalidPaginationLimit { requested: u16, max: u16 },
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
            ApiError::InvalidPaginationLimit { .. } => {
                ApiErrorResponse::BadRequestWithMessage(value.to_string())
            }
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
