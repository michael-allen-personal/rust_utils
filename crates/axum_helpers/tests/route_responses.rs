//! Response-code assertions for the route handlers.
//!
//! The other test crates check that handlers compile and mount; this one checks what they
//! actually answer. `PgPool::connect_lazy` builds a pool without opening a connection, and
//! the fixture's SQL impls never touch it, so a handler can be called directly and its
//! `Response` inspected without a database.

use axum_helpers::async_trait::async_trait;
use axum_helpers::axum::extract::{Json, Path, State};
use axum_helpers::axum::http::StatusCode;
use axum_helpers::sql_traits::{
    HasPrimaryKey, HasRequestBody, HasUpdateFields, ReplaceRecord, UpdateFields, UpdateRecord,
};
use axum_helpers::sqlx::{self, PgPool};
use axum_helpers::{ReplaceRoute, UpdateRoute, serde};

/// The primary key the fixture treats as matching no row.
const MISSING_ID: i64 = 404;

#[derive(serde::Serialize)]
#[serde(crate = "axum_helpers::serde")]
struct Widget {
    id: i64,
    name: String,
}

#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct WidgetBody {
    name: String,
}

impl HasPrimaryKey for Widget {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

impl HasRequestBody for Widget {
    type RequestBody = WidgetBody;
    fn from_request_body(body: WidgetBody, primary_key: i64) -> Self {
        Widget {
            id: primary_key,
            name: body.name,
        }
    }
}

#[async_trait]
impl ReplaceRecord for Widget {
    async fn replace_record(self, _pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        if self.id == MISSING_ID {
            return Ok(None);
        }
        Ok(Some(self))
    }
}

impl ReplaceRoute for Widget {}

/// A pool that is never connected to. `connect_lazy` defers the connection until first
/// use, and nothing in these fixtures uses it.
fn pool() -> PgPool {
    PgPool::connect_lazy("postgres://localhost/unused").expect("a valid connection string")
}

fn body() -> WidgetBody {
    WidgetBody {
        name: "cog".to_string(),
    }
}

#[tokio::test]
async fn replace_answers_404_when_the_key_matches_no_row() {
    let response = Widget::replace_route(State(pool()), Path(MISSING_ID), Json(body())).await;

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "a missing row must not surface as a 500"
    );
}

#[tokio::test]
async fn replace_answers_200_when_the_row_exists() {
    let response = Widget::replace_route(State(pool()), Path(1), Json(body())).await;

    assert_eq!(response.status(), StatusCode::OK);
}

// --- UpdateRoute --------------------------------------------------------------------

/// `Widget`'s non-key fields, each optional. Hand-written for the same reason the rest of
/// this file is: the subject here is what the handler answers, not what the derive emits.
#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct WidgetUpdate {
    #[serde(default)]
    name: Option<String>,
}

impl HasUpdateFields for Widget {
    type UpdateFields = WidgetUpdate;

    fn apply_update_fields(mut record: Self, fields: WidgetUpdate) -> Self {
        if let Some(name) = fields.name {
            record.name = name;
        }
        record
    }
}

impl UpdateFields for WidgetUpdate {
    type Record = Widget;

    fn is_empty(&self) -> bool {
        self.name.is_none()
    }
}

#[async_trait]
impl UpdateRecord for Widget {
    async fn update_record(
        _pool: &PgPool,
        primary_key: i64,
        update_fields: WidgetUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        if primary_key == MISSING_ID {
            return Ok(None);
        }
        Ok(Some(update_fields.apply(Widget {
            id: primary_key,
            name: "before".to_string(),
        })))
    }
}

impl UpdateRoute for Widget {}

fn update(name: Option<&str>) -> WidgetUpdate {
    WidgetUpdate {
        name: name.map(str::to_string),
    }
}

/// An empty `SET` list is a SQL syntax error rather than a no-op, so the route rejects an
/// empty patch before it can reach an implementation that builds one.
#[tokio::test]
async fn update_answers_400_when_no_field_is_set() {
    let response = Widget::update_route(State(pool()), Path(1), Json(update(None))).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_answers_404_when_the_key_matches_no_row() {
    let response =
        Widget::update_route(State(pool()), Path(MISSING_ID), Json(update(Some("after")))).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_answers_200_when_a_field_is_set() {
    let response = Widget::update_route(State(pool()), Path(1), Json(update(Some("after")))).await;

    assert_eq!(response.status(), StatusCode::OK);
}

/// The empty-patch check has to run before the key is looked up: an empty patch is a bad
/// request whether or not the row exists, and it must not cost a database round trip.
#[tokio::test]
async fn an_empty_patch_is_rejected_before_the_key_is_looked_up() {
    let response = Widget::update_route(State(pool()), Path(MISSING_ID), Json(update(None))).await;

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "the empty check must precede the lookup, so 400 wins over 404"
    );
}
