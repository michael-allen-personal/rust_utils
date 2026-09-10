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
use axum_helpers::{GetRecordRoute, ReplaceRoute, UpdateRoute, serde};

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

// --- Composite primary keys ------------------------------------------------------------
//
// The one place URL-segment binding is actually exercised. Every test above calls a handler
// directly with a `Path` built by hand, which skips the extractor entirely — so none of them
// can tell which segment filled which field. These drive a real `Router` with a real request.

/// Marked fields in the order `user_id`, `group_id`; every route below deliberately declares
/// its segments in the opposite order. Under the old tuple key that swapped the two silently.
#[derive(serde::Serialize, macros::Record, macros::GetRecordRoute)]
#[macros(body_derive(axum_helpers::serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
struct Membership {
    #[macros(primary_key)]
    user_id: i64,
    #[macros(primary_key)]
    group_id: i64,
    role: String,
}

/// Echoes the key it was handed straight back, so the response body says exactly which value
/// landed in which field.
#[async_trait]
impl axum_helpers::sql_traits::GetRecord for Membership {
    async fn get_record(
        _pool: &PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(Some(Membership {
            user_id: primary_key.user_id,
            group_id: primary_key.group_id,
            role: "member".to_string(),
        }))
    }
}

fn memberships(path: &str) -> axum_helpers::axum::Router {
    axum_helpers::axum::Router::new()
        .route(
            path,
            axum_helpers::axum::routing::get(Membership::get_record_route),
        )
        .with_state(pool())
}

/// Sends one request through the router and returns the status and the body as a string.
async fn get(path: &str, uri: &str) -> (axum_helpers::axum::http::StatusCode, String) {
    use axum_helpers::axum::body::{Body, to_bytes};
    use axum_helpers::axum::http::Request;
    use tower::ServiceExt as _;

    let response = memberships(path)
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router is infallible");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a complete body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

/// The bug this key struct exists to fix. The route names `{group_id}` first, so a tuple key
/// filled `user_id` from `9` and `group_id` from `4` — addressing a row nobody asked for, with
/// no error anywhere. Binding by name makes segment order irrelevant.
#[tokio::test]
async fn a_composite_key_binds_path_segments_by_name_not_by_position() {
    let (status, body) = get("/m/{group_id}/{user_id}", "/m/9/4").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""user_id":4"#) && body.contains(r#""group_id":9"#),
        "each segment must fill the field it is named after, got {body}"
    );
}

/// Declaration order still works, of course — the point is that neither order is special.
#[tokio::test]
async fn declaration_order_binds_the_same_way() {
    let (status, body) = get("/m/{user_id}/{group_id}", "/m/4/9").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""user_id":4"#) && body.contains(r#""group_id":9"#),
        "got {body}"
    );
}

/// A route may capture more than the key: `org_id` names no field, so it is ignored rather
/// than rejected, which is what lets a keyed route nest under an unrelated segment.
#[tokio::test]
async fn a_segment_the_key_has_no_field_for_is_ignored() {
    let (status, body) = get("/orgs/{org_id}/m/{group_id}/{user_id}", "/orgs/1/m/9/4").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""user_id":4"#) && body.contains(r#""group_id":9"#),
        "got {body}"
    );
}

/// A key field no segment names cannot be filled, and the extractor says so before the
/// handler runs. Loud and at the right time — the failure the tuple key had here was a
/// silently wrong row.
#[tokio::test]
async fn a_misspelled_segment_is_rejected_rather_than_guessed_at() {
    let (status, _) = get("/m/{group_id}/{userid}", "/m/9/4").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// --- Pagination limit errors -----------------------------------------------------------
//
// The limit check is the only error this crate raises that carries data, so it is the only
// one whose message can be wrong while its status is right. Both are asserted.

/// A narrow error widens into `ApiError` with `?`, which is the reason it is a subset rather
/// than an inline variant: `validate` returns only what it can actually produce.
#[test]
fn a_request_error_widens_into_an_api_error() {
    /// Shaped like a real handler: the narrow error is raised with `?` and the signature
    /// widens it, which is the only thing being asserted.
    fn widen(limit: u16) -> Result<(), axum_helpers::ApiError> {
        if limit > 100 {
            Err(axum_helpers::RequestError::InvalidPaginationLimit {
                requested: limit,
                max: 100,
            })?;
        }
        Ok(())
    }

    assert!(widen(100).is_ok(), "the maximum itself is not an error");
    assert!(matches!(
        widen(500),
        Err(axum_helpers::ApiError::InvalidPaginationLimit {
            requested: 500,
            max: 100
        })
    ));
}

#[tokio::test]
async fn an_invalid_limit_answers_400_naming_the_maximum_and_the_request() {
    use axum_helpers::axum::body::to_bytes;
    use axum_helpers::axum::response::IntoResponse;

    let response = axum_helpers::ApiError::InvalidPaginationLimit {
        requested: 500,
        max: 100,
    }
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a complete body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf-8");

    assert!(
        body.contains("100") && body.contains("500"),
        "the message must name both the maximum and what was asked for, got {body}"
    );
    assert!(
        body.starts_with(r#"{"message":"#),
        "every error body in this crate is a message object, got {body}"
    );
}

// --- Paginated list responses ----------------------------------------------------------
//
// Driven through a real `Router`, because a query string only exists in a real request: a
// handler called directly takes a `Query` built by hand, which proves nothing about what a URL
// deserializes into.

/// The offset past which the fixture reports an empty page, so an empty result can be driven
/// through a real request.
const PAST_THE_END: u32 = 1000;

/// Echoes the parameters it received into the data, so the response body says exactly what
/// reached the SQL layer. Cheaper and clearer than giving the fixture interior mutability.
#[async_trait]
impl axum_helpers::sql_traits::ListRecordsPaginated<axum_helpers::sql_traits::OffsetParams>
    for Widget
{
    async fn list_records_paginated(
        _pool: &PgPool,
        params: axum_helpers::sql_traits::OffsetParams,
    ) -> Result<
        axum_helpers::sql_traits::Page<Self, axum_helpers::sql_traits::OffsetPagination>,
        sqlx::Error,
    > {
        // The limit and offset that arrived, readable straight off the response body.
        let data = if params.offset >= PAST_THE_END {
            Vec::new()
        } else {
            vec![Widget {
                id: i64::from(params.limit),
                name: params.offset.to_string(),
            }]
        };

        Ok(axum_helpers::sql_traits::Page {
            data,
            pagination: axum_helpers::sql_traits::OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: params.include_total.then_some(1234),
            },
        })
    }
}

/// A maximum below the crate default of 50, so a bare request also proves the default comes
/// from this type rather than from any crate-wide number.
impl axum_helpers::ListRecordsPaginatedRoute<axum_helpers::OffsetParamsQuery<10, 25>> for Widget {}

fn paged_router() -> axum_helpers::axum::Router {
    axum_helpers::axum::Router::new()
        .route(
            "/widgets",
            axum_helpers::axum::routing::get(
                <Widget as axum_helpers::ListRecordsPaginatedRoute<
                    axum_helpers::OffsetParamsQuery<10, 25>,
                >>::list_records_paginated_route,
            ),
        )
        .with_state(pool())
}

/// Sends one request through the paginated router and returns the status and body.
async fn paged(uri: &str) -> (StatusCode, String) {
    use axum_helpers::axum::body::{Body, to_bytes};
    use axum_helpers::axum::http::Request;
    use tower::ServiceExt as _;

    let response = paged_router()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router is infallible");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a complete body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

#[tokio::test]
async fn a_request_naming_no_parameters_reaches_the_impl_with_this_types_default() {
    let (status, body) = paged("/widgets").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""id":10"#) && body.contains(r#""name":"0""#),
        "a bare request must arrive as limit 10, offset 0, got {body}"
    );
}

#[tokio::test]
async fn a_named_limit_and_offset_reach_the_impl() {
    let (status, body) = paged("/widgets?limit=5&offset=40").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""id":5"#) && body.contains(r#""name":"40""#),
        "got {body}"
    );
}

#[tokio::test]
async fn the_envelope_is_data_plus_a_nested_pagination_object() {
    let (_, body) = paged("/widgets?limit=5&offset=40").await;

    assert!(
        body.starts_with(r#"{"data":["#) && body.contains(r#""pagination":{"#),
        "the response must be an envelope, not a bare array, got {body}"
    );
}

#[tokio::test]
async fn an_unrequested_total_is_null_and_a_requested_one_is_a_number() {
    let (_, without) = paged("/widgets").await;
    assert!(
        without.contains(r#""total":null"#),
        "a total nobody asked for must be null, got {without}"
    );

    let (_, with) = paged("/widgets?include_total=true").await;
    assert!(
        with.contains(r#""total":1234"#),
        "a requested total must be filled, got {with}"
    );
}

#[tokio::test]
async fn a_limit_of_zero_is_rejected() {
    let (status, body) = paged("/widgets?limit=0").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.starts_with(r#"{"message":"#), "got {body}");
}

#[tokio::test]
async fn a_limit_above_the_maximum_is_rejected_and_the_maximum_is_inclusive() {
    let (over, _) = paged("/widgets?limit=26").await;
    assert_eq!(over, StatusCode::BAD_REQUEST);

    let (at, _) = paged("/widgets?limit=25").await;
    assert_eq!(at, StatusCode::OK, "the maximum itself must be allowed");
}

/// An empty page is still a page. `GetLatestRoute` answers `204` for an empty table, but a list
/// that matched nothing is a successful list of nothing, and a client parsing the envelope must
/// not have to handle a bodyless response as a special case.
#[tokio::test]
async fn an_empty_page_is_200_with_a_full_envelope_not_204() {
    let (status, body) = paged(&format!("/widgets?offset={PAST_THE_END}")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""data":[]"#) && body.contains(r#""pagination":{"#),
        "an empty page keeps the envelope, got {body}"
    );
}

/// The reason the handler extracts a `Result` rather than a bare `Query`: axum's own rejection
/// body is plain text, and every error in this crate is a message object.
#[tokio::test]
async fn an_unparseable_limit_is_a_400_in_this_crates_error_shape() {
    let (status, body) = paged("/widgets?limit=abc").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.starts_with(r#"{"message":"#),
        "a query rejection must be rendered like every other error, got {body}"
    );
}

// --- The paginated *Where route ---------------------------------------------------------
//
// One request has to fill a path segment and a query string at once, which is the only thing
// this route adds over the unfiltered one.

/// The filter the route extracts from the path, serving as its own `PathParams` through the
/// blanket `From<T> for T`.
#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct OwnerFilter {
    owner_id: i64,
}

#[async_trait]
impl
    axum_helpers::sql_traits::ListRecordsWherePaginated<
        OwnerFilter,
        axum_helpers::sql_traits::OffsetParams,
    > for Widget
{
    async fn list_records_where_paginated(
        _pool: &PgPool,
        where_params: OwnerFilter,
        params: axum_helpers::sql_traits::OffsetParams,
    ) -> Result<
        axum_helpers::sql_traits::Page<Self, axum_helpers::sql_traits::OffsetPagination>,
        sqlx::Error,
    > {
        Ok(axum_helpers::sql_traits::Page {
            // The filter's value and the limit, both readable off the body.
            data: vec![Widget {
                id: where_params.owner_id,
                name: params.limit.to_string(),
            }],
            pagination: axum_helpers::sql_traits::OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: None,
            },
        })
    }
}

impl
    axum_helpers::ListRecordsWherePaginatedRoute<
        OwnerFilter,
        axum_helpers::OffsetParamsQuery<10, 25>,
    > for Widget
{
    type PathParams = OwnerFilter;
}

async fn owner_paged(uri: &str) -> (StatusCode, String) {
    use axum_helpers::axum::body::{Body, to_bytes};
    use axum_helpers::axum::http::Request;
    use tower::ServiceExt as _;

    let router = axum_helpers::axum::Router::new()
        .route(
            "/owners/{owner_id}/widgets",
            axum_helpers::axum::routing::get(
                <Widget as axum_helpers::ListRecordsWherePaginatedRoute<
                    OwnerFilter,
                    axum_helpers::OffsetParamsQuery<10, 25>,
                >>::list_records_where_paginated_route,
            ),
        )
        .with_state(pool());

    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router is infallible");

    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a complete body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf-8"))
}

#[tokio::test]
async fn a_paginated_where_route_binds_the_path_and_the_query_from_one_request() {
    let (status, body) = owner_paged("/owners/7/widgets?limit=5").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(r#""id":7"#) && body.contains(r#""name":"5""#),
        "the path segment must fill the filter and the query string the limit, got {body}"
    );
}

#[tokio::test]
async fn a_paginated_where_route_still_defaults_the_limit() {
    let (status, body) = owner_paged("/owners/7/widgets").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(r#""name":"10""#), "got {body}");
}

#[tokio::test]
async fn a_paginated_where_route_rejects_an_over_max_limit() {
    let (status, _) = owner_paged("/owners/7/widgets?limit=26").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}
