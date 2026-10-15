//! Response-code assertions for the route handlers.
//!
//! The other test crates check that handlers compile and mount; this one checks what they
//! actually answer. `Widget`'s fixture runs against a real in-memory SQLite database, built
//! fresh by `pool()` for every test, so the assertions below hold for rows that are actually
//! read from and written to a database rather than for a fake that ignores its pool.
//! `Membership` and the paginated fixtures still fake their SQL impls, because their subject
//! is axum's own extraction (composite-key path binding, query-parameter plumbing) rather
//! than the query itself — but they share the same real pool for type-level consistency.

use axum_helpers::async_trait::async_trait;
use axum_helpers::axum::extract::{Json, Path, State};
use axum_helpers::axum::http::StatusCode;
use axum_helpers::sql_traits::{
    GetRecord, HasPrimaryKey, HasRequestBody, HasUpdateFields, ReplaceRecord, UpdateFields,
    UpdateRecord,
};
use axum_helpers::sqlx::{self, Pool, Sqlite};
use axum_helpers::{GetRecordRoute, ReplaceRoute, UpdateRoute, serde};

/// The primary key the fixture treats as matching no row.
const MISSING_ID: i64 = 404;

#[derive(serde::Serialize, sql_traits::Database)]
#[sql_traits(database = Sqlite)]
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
impl GetRecord for Widget {
    async fn get_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM widget WHERE id = ?")
            .bind(primary_key)
            .fetch_optional(pool)
            .await
            .map(|row| row.map(|(id, name)| Widget { id, name }))
    }
}

impl GetRecordRoute for Widget {}

#[async_trait]
impl ReplaceRecord for Widget {
    async fn replace_record(self, pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, (i64, String)>(
            "UPDATE widget SET name = ? WHERE id = ? RETURNING id, name",
        )
        .bind(&self.name)
        .bind(self.id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(|(id, name)| Widget { id, name }))
    }
}

impl ReplaceRoute for Widget {}

/// A real in-memory SQLite database with this file's schema already created.
///
/// `sqlx` shares one in-memory database across a pool's connections, so no
/// `max_connections(1)` or shared-cache URI is needed — a table created here is visible to
/// every connection the pool hands out. Each call builds a fresh database, so tests cannot
/// see one another's rows.
async fn pool() -> Pool<Sqlite> {
    let pool = Pool::<Sqlite>::connect("sqlite::memory:")
        .await
        .expect("an in-memory database");
    sqlx::query("CREATE TABLE widget (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .expect("the schema");
    pool
}

/// Inserts one row and returns its key, for tests that need a record to address.
async fn insert_widget(pool: &Pool<Sqlite>, name: &str) -> i64 {
    sqlx::query_as::<_, (i64,)>("INSERT INTO widget (name) VALUES (?) RETURNING id")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("the insert")
        .0
}

#[tokio::test]
async fn get_answers_200_with_the_row_that_is_actually_stored() {
    use axum_helpers::axum::body::to_bytes;

    let pool = pool().await;
    let id = insert_widget(&pool, "cog").await;

    let response = Widget::get_record_route(State(pool), Path(id)).await;

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a complete body");
    let body = String::from_utf8(bytes.to_vec()).expect("utf-8");

    assert!(
        body.contains(&format!(r#""id":{id}"#)) && body.contains(r#""name":"cog""#),
        "the response must carry the row that was actually inserted, got {body}"
    );
}

fn body() -> WidgetBody {
    WidgetBody {
        name: "cog".to_string(),
    }
}

#[tokio::test]
async fn replace_answers_404_when_the_key_matches_no_row() {
    let pool = pool().await;

    let response = Widget::replace_route(State(pool), Path(MISSING_ID), Json(body())).await;

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "a missing row must not surface as a 500"
    );
}

#[tokio::test]
async fn replace_answers_200_when_the_row_exists() {
    let pool = pool().await;
    let id = insert_widget(&pool, "cog").await;

    let response = Widget::replace_route(State(pool), Path(id), Json(body())).await;

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
        pool: &Pool<Sqlite>,
        primary_key: i64,
        update_fields: WidgetUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        // Fetch-apply-replace, which is one of the two shapes `UpdateRecord`'s own docs
        // describe. It keeps the statement static, and `UpdateRoute` has already rejected
        // an empty update with `400` before this is reached.
        let Some(record) = Self::get_record(pool, primary_key).await? else {
            return Ok(None);
        };
        update_fields.apply(record).replace_record(pool).await
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
    let pool = pool().await;
    let id = insert_widget(&pool, "before").await;

    let response = Widget::update_route(State(pool), Path(id), Json(update(None))).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn update_answers_404_when_the_key_matches_no_row() {
    let pool = pool().await;

    let response =
        Widget::update_route(State(pool), Path(MISSING_ID), Json(update(Some("after")))).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn update_answers_200_when_a_field_is_set() {
    let pool = pool().await;
    let id = insert_widget(&pool, "before").await;

    let response = Widget::update_route(State(pool), Path(id), Json(update(Some("after")))).await;

    assert_eq!(response.status(), StatusCode::OK);
}

/// The empty-patch check has to run before the key is looked up: an empty patch is a bad
/// request whether or not the row exists, and it must not cost a database round trip.
#[tokio::test]
async fn an_empty_patch_is_rejected_before_the_key_is_looked_up() {
    let pool = pool().await;

    let response = Widget::update_route(State(pool), Path(MISSING_ID), Json(update(None))).await;

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
#[derive(
    serde::Serialize, sql_traits::Record, axum_helpers::GetRecordRoute, sql_traits::Database,
)]
#[sql_traits(body_derive(axum_helpers::serde::Deserialize))]
#[sql_traits(database = Sqlite)]
#[serde(crate = "axum_helpers::serde")]
struct Membership {
    #[sql_traits(primary_key)]
    user_id: i64,
    #[sql_traits(primary_key)]
    group_id: i64,
    role: String,
}

/// Echoes the key it was handed straight back, so the response body says exactly which value
/// landed in which field. Still a fake: these tests assert axum's composite-key path binding
/// by name, not a query, so a real second table would not strengthen them.
#[async_trait]
impl axum_helpers::sql_traits::GetRecord for Membership {
    async fn get_record(
        _pool: &Pool<Sqlite>,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(Some(Membership {
            user_id: primary_key.user_id,
            group_id: primary_key.group_id,
            role: "member".to_string(),
        }))
    }
}

async fn memberships(path: &str) -> axum_helpers::axum::Router {
    axum_helpers::axum::Router::new()
        .route(
            path,
            axum_helpers::axum::routing::get(Membership::get_record_route),
        )
        .with_state(pool().await)
}

/// Sends one request through the router and returns the status and the body as a string.
async fn get(path: &str, uri: &str) -> (axum_helpers::axum::http::StatusCode, String) {
    use axum_helpers::axum::body::{Body, to_bytes};
    use axum_helpers::axum::http::Request;
    use tower::ServiceExt as _;

    let response = memberships(path)
        .await
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
            Err(axum_helpers::ValidationError::InvalidPaginationLimit {
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

/// The subsets render themselves, so a consumer's own handler can return the narrow error it
/// actually produces instead of widening by hand. Each must land on the same status its
/// variants get through `ApiError`, which is the thing that could silently drift: these go
/// through their own `IntoResponse`, not the one the route handlers use.
#[tokio::test]
async fn each_error_subset_renders_itself_at_the_status_its_variants_get() {
    use axum_helpers::axum::response::IntoResponse;

    let request_error = axum_helpers::ValidationError::InvalidPaginationLimit {
        requested: 500,
        max: 100,
    }
    .into_response();
    assert_eq!(
        request_error.status(),
        StatusCode::BAD_REQUEST,
        "a client-caused error is a 400 whether it renders itself or goes through ApiError"
    );

    let io_error = axum_helpers::IOError::Sql(sqlx::Error::RowNotFound).into_response();
    assert_eq!(
        io_error.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "every sqlx error is a 500, this path included"
    );

    let io_error = axum_helpers::IOError::Serde(serde_error()).into_response();
    assert_eq!(
        io_error.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "every serde error is a 500, this path included"
    );
}

/// A `serde_json::Error` to render, built the only way one can be: by asking `serde_json` to
/// parse something it cannot. Its *shape* is a deserialization failure, which is the point —
/// see [`a_serde_error_is_a_500_even_though_it_reads_like_a_400`].
fn serde_error() -> axum_helpers::serde_json::Error {
    axum_helpers::serde_json::from_str::<i64>("not a number")
        .expect_err("`not a number` is not an i64")
}

/// The status that is easiest to get wrong, because the variant reads like the client's fault.
/// Nothing a client sends reaches it: a JSON body fails in axum's `Json` extractor as its own
/// `400` before the handler runs, and a query string arrives as `InvalidQueryParams`. So even
/// a deserialization-shaped `serde_json::Error` here is the server failing to serialize
/// something of its own, and a `500` is the honest answer. Asserted through `ApiError`, where
/// the decision is made, as well as through the subset above.
#[tokio::test]
async fn a_serde_error_is_a_500_even_though_it_reads_like_a_400() {
    use axum_helpers::axum::response::IntoResponse;

    let response = axum_helpers::ApiError::Serde(serde_error()).into_response();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
/// Still a fake: its subject is query-parameter plumbing reaching the impl, not SQL — real
/// paginated queries are a separate task, in `sql_traits`.
#[async_trait]
impl axum_helpers::sql_traits::ListRecordsPaginated<axum_helpers::sql_traits::OffsetParams>
    for Widget
{
    async fn list_records_paginated(
        _pool: &Pool<Sqlite>,
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

async fn paged_router() -> axum_helpers::axum::Router {
    axum_helpers::axum::Router::new()
        .route(
            "/widgets",
            axum_helpers::axum::routing::get(
                <Widget as axum_helpers::ListRecordsPaginatedRoute<
                    axum_helpers::OffsetParamsQuery<10, 25>,
                >>::list_records_paginated_route,
            ),
        )
        .with_state(pool().await)
}

/// Sends one request through the paginated router and returns the status and body.
async fn paged(uri: &str) -> (StatusCode, String) {
    use axum_helpers::axum::body::{Body, to_bytes};
    use axum_helpers::axum::http::Request;
    use tower::ServiceExt as _;

    let response = paged_router()
        .await
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

/// The body is asserted here, not just the status: a handler answering a bare
/// `ApiErrorResponse::BadRequest` would satisfy the status alone, and a caller told only "bad
/// request" cannot tell what limit it should have asked for. The numbers are what close the loop
/// from the URL through `validate` to the rendered message.
#[tokio::test]
async fn a_limit_above_the_maximum_is_rejected_and_the_maximum_is_inclusive() {
    let (over, body) = paged("/widgets?limit=26").await;
    assert_eq!(over, StatusCode::BAD_REQUEST);
    assert!(
        body.contains("26") && body.contains("25"),
        "the message must name what was asked for and this route's maximum, got {body}"
    );

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

/// The filter the route extracts from the path, serving as its own `WhereParams` through the
/// blanket `From<T> for T`.
#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct OwnerFilter {
    owner_id: i64,
}

/// Still a fake: its subject is query-parameter plumbing reaching the impl, not SQL — real
/// paginated queries are a separate task, in `sql_traits`.
#[async_trait]
impl
    axum_helpers::sql_traits::ListRecordsWherePaginated<
        OwnerFilter,
        axum_helpers::sql_traits::OffsetParams,
    > for Widget
{
    async fn list_records_where_paginated(
        _pool: &Pool<Sqlite>,
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
    type WhereParams = OwnerFilter;
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
        .with_state(pool().await);

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

#[tokio::test]
async fn a_paginated_where_route_will_not_let_the_query_override_the_path() {
    let (status, body) = owner_paged("/owners/7/widgets?owner_id=8&limit=5").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.starts_with(r#"{"message":"#) && body.contains("duplicate field `owner_id`"),
        "a key in both the path and the query must be rejected, not resolved, got {body}"
    );
}

// --- Mutation responses ----------------------------------------------------------------
//
// Whether a create or delete answers with a body is decided by what its `ReturnType`
// serializes to, so each case needs its own `ReturnType` — and a type has only one per SQL
// trait, hence one small fixture per shape. All fake their SQL impls: the subject is how a
// returned value is rendered, not the query that produced it. Handlers are called directly,
// since no URL-segment binding is under test here.

use axum_helpers::axum::response::Response;
use axum_helpers::sql_traits::{BulkInsertRecords, DeleteRecord, DeleteRecordsWhere, InsertRecord};
use axum_helpers::{BulkCreateRoute, CreateRoute, DeleteRecordsWhereRoute, DeleteRoute};

/// The key the mutation fixtures treat as failing with a database error.
const FAILING_ID: i64 = 500;

/// Status, `Content-Type` and body of a rendered response.
async fn rendered(response: Response) -> (StatusCode, Option<String>, String) {
    use axum_helpers::axum::body::to_bytes;
    use axum_helpers::axum::http::header::CONTENT_TYPE;

    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .map(|value| value.to_str().expect("an ascii header").to_owned());
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a complete body");
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).expect("utf-8"),
    )
}

/// Every mutation returns a value worth reporting: a delete counts rows, a bulk insert
/// returns the assigned keys, and a filtered delete returns `()` — the one shape here with
/// nothing to say.
#[derive(serde::Deserialize, sql_traits::Database)]
#[sql_traits(database = Sqlite)]
#[serde(crate = "axum_helpers::serde")]
struct Counted {
    id: i64,
}

impl HasPrimaryKey for Counted {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl DeleteRecord for Counted {
    type ReturnType = u64;
    async fn delete_record(_pool: &Pool<Sqlite>, _primary_key: i64) -> Result<u64, sqlx::Error> {
        Ok(3)
    }
}

impl DeleteRoute for Counted {}

#[async_trait]
impl BulkInsertRecords for Counted {
    type ReturnType = Vec<i64>;
    async fn bulk_insert_records(
        _pool: &Pool<Sqlite>,
        records: &[Self],
    ) -> Result<Vec<i64>, sqlx::Error> {
        Ok(records.iter().map(|record| record.id).collect())
    }
}

impl BulkCreateRoute<'_> for Counted {}

#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct CountedFilter {
    #[allow(dead_code)]
    id: i64,
}

#[async_trait]
impl DeleteRecordsWhere<CountedFilter> for Counted {
    type ReturnType = ();
    async fn delete_records_where(
        _pool: &Pool<Sqlite>,
        _where_params: CountedFilter,
    ) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

impl DeleteRecordsWhereRoute<CountedFilter> for Counted {
    type WhereParams = CountedFilter;
}

/// The opposite shape: a delete and an insert that both return `()`.
#[derive(serde::Deserialize, sql_traits::Database)]
#[sql_traits(database = Sqlite)]
#[serde(crate = "axum_helpers::serde")]
struct Silent {
    id: i64,
}

impl HasPrimaryKey for Silent {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl DeleteRecord for Silent {
    type ReturnType = ();
    async fn delete_record(_pool: &Pool<Sqlite>, _primary_key: i64) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

impl DeleteRoute for Silent {}

#[async_trait]
impl InsertRecord for Silent {
    type ReturnType = ();
    async fn insert_record(self, _pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

impl CreateRoute<'_> for Silent {}

/// A delete returning `Option<u64>`: `None` for `MISSING_ID`, `Some(1)` otherwise.
#[derive(sql_traits::Database)]
#[sql_traits(database = Sqlite)]
struct Maybe {
    id: i64,
}

impl HasPrimaryKey for Maybe {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl DeleteRecord for Maybe {
    type ReturnType = Option<u64>;
    async fn delete_record(
        _pool: &Pool<Sqlite>,
        primary_key: i64,
    ) -> Result<Option<u64>, sqlx::Error> {
        Ok((primary_key != MISSING_ID).then_some(1))
    }
}

impl DeleteRoute for Maybe {}

/// Overrides the response hook, and fails with a database error for `FAILING_ID`.
#[derive(sql_traits::Database)]
#[sql_traits(database = Sqlite)]
struct Overridden {
    id: i64,
}

impl HasPrimaryKey for Overridden {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl DeleteRecord for Overridden {
    type ReturnType = u64;
    async fn delete_record(_pool: &Pool<Sqlite>, primary_key: i64) -> Result<u64, sqlx::Error> {
        if primary_key == FAILING_ID {
            Err(sqlx::Error::RowNotFound)
        } else {
            Ok(1)
        }
    }
}

impl DeleteRoute for Overridden {
    fn delete_response(rows_affected: u64) -> Response {
        use axum_helpers::axum::response::IntoResponse;

        (StatusCode::ACCEPTED, format!("overridden: {rows_affected}")).into_response()
    }
}

#[tokio::test]
async fn a_delete_returning_a_count_answers_200_with_the_count_as_json() {
    let response = Counted::delete_route(State(pool().await), Path(1)).await;

    let (status, content_type, body) = rendered(response).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert_eq!(body, "3");
}

#[tokio::test]
async fn a_delete_returning_unit_answers_204_with_no_body() {
    let response = Silent::delete_route(State(pool().await), Path(1)).await;

    let (status, content_type, body) = rendered(response).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(content_type, None);
    assert_eq!(body, "");
}

/// `None` is a `204` like `()`, never a `404`: the null rule reads the serialized value, not
/// what an `Option` might mean to a particular implementation.
#[tokio::test]
async fn a_delete_returning_none_answers_204_not_404() {
    let missing = Maybe::delete_route(State(pool().await), Path(MISSING_ID)).await;
    let present = Maybe::delete_route(State(pool().await), Path(1)).await;

    let (status, _, body) = rendered(missing).await;
    assert_eq!((status, body.as_str()), (StatusCode::NO_CONTENT, ""));
    let (status, _, body) = rendered(present).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "1"));
}

#[tokio::test]
async fn a_filtered_delete_returning_unit_answers_204_not_200_null() {
    let response = <Counted as DeleteRecordsWhereRoute<CountedFilter>>::delete_records_where_route(
        State(pool().await),
        PathAndQuery(CountedFilter { id: 1 }),
    )
    .await;

    let (status, _, body) = rendered(response).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, "");
}

#[tokio::test]
async fn a_create_returning_unit_answers_201_with_no_body_not_null() {
    let response = Silent::create_route(State(pool().await), Json(Silent { id: 1 })).await;

    let (status, content_type, body) = rendered(response).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(content_type, None);
    assert_eq!(body, "");
}

#[tokio::test]
async fn a_bulk_create_returning_values_answers_201_with_them_as_json() {
    let response = Counted::bulk_create_route(
        State(pool().await),
        Json(vec![Counted { id: 1 }, Counted { id: 2 }]),
    )
    .await;

    let (status, content_type, body) = rendered(response).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(content_type.as_deref(), Some("application/json"));
    assert_eq!(body, "[1,2]");
}

#[tokio::test]
async fn an_overridden_hook_renders_the_success() {
    let response = Overridden::delete_route(State(pool().await), Path(1)).await;

    let (status, _, body) = rendered(response).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, "overridden: 1");
}

/// The hook only ever sees a success, so no override can change the error shape: a failed
/// delete is still this crate's `500` with a `{"message": …}` body.
#[tokio::test]
async fn an_overridden_hook_never_sees_an_error() {
    let response = Overridden::delete_route(State(pool().await), Path(FAILING_ID)).await;

    let (status, _, body) = rendered(response).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains(r#""message""#), "got {body}");
}

// --- PathAndQuery ----------------------------------------------------------------------
//
// The extractor every `*WhereRoute` reads its filter through, driven here through a
// hand-written handler that echoes what it extracted. Its subject is which source fills
// which field, so a real `Router` is the only honest vantage point: it is what decides which
// segments a request carries.

use axum_helpers::PathAndQuery;

/// `owner_id` is required and the other two are optional, so one fixture can show a
/// required field filled from either source and an optional one left out.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct Probe {
    owner_id: i64,
    colour: Option<String>,
    size: Option<u8>,
}

/// A single string field, for checking what a path segment's value survives.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct Named {
    name: String,
}

async fn probe(uri: &str) -> (StatusCode, String) {
    use axum_helpers::axum::Json;
    use axum_helpers::axum::body::Body;
    use axum_helpers::axum::http::Request;
    use axum_helpers::axum::routing::get;
    use tower::ServiceExt as _;

    async fn echo_probe(PathAndQuery(probe): PathAndQuery<Probe>) -> Json<Probe> {
        Json(probe)
    }
    async fn echo_named(PathAndQuery(named): PathAndQuery<Named>) -> Json<Named> {
        Json(named)
    }

    let router = axum_helpers::axum::Router::new()
        .route("/owners/{owner_id}/probes", get(echo_probe))
        .route("/probes", get(echo_probe))
        .route("/named/{name}", get(echo_named));

    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("a valid request"),
        )
        .await
        .expect("the router is infallible");
    let (status, _, body) = rendered(response).await;
    (status, body)
}

#[tokio::test]
async fn one_type_is_filled_from_the_path_and_the_query_together() {
    let (status, body) = probe("/owners/7/probes?colour=red&size=3").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"owner_id":7,"colour":"red","size":3}"#);
}

#[tokio::test]
async fn an_absent_query_string_leaves_the_optional_fields_none() {
    let (status, body) = probe("/owners/7/probes").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"owner_id":7,"colour":null,"size":null}"#);
}

#[tokio::test]
async fn a_route_with_no_segments_fills_everything_from_the_query() {
    let (status, body) = probe("/probes?owner_id=3&colour=blue").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"owner_id":3,"colour":"blue","size":null}"#);
}

#[tokio::test]
async fn a_required_field_neither_source_supplies_is_a_400_in_this_crates_error_shape() {
    let (status, body) = probe("/probes?colour=blue").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.starts_with(r#"{"message":"#) && body.contains("owner_id"),
        "got {body}"
    );
}

#[tokio::test]
async fn a_key_in_both_the_path_and_the_query_is_a_400_not_an_override() {
    let (status, body) = probe("/owners/7/probes?owner_id=8").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body.starts_with(r#"{"message":"#) && body.contains("duplicate field `owner_id`"),
        "got {body}"
    );
}

#[tokio::test]
async fn an_unparseable_query_value_is_a_400_in_this_crates_error_shape() {
    let (status, body) = probe("/owners/7/probes?size=big").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.starts_with(r#"{"message":"#), "got {body}");
}

#[tokio::test]
async fn an_unparseable_path_value_is_a_400_too() {
    let (status, body) = probe("/owners/seven/probes").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.starts_with(r#"{"message":"#), "got {body}");
}

/// The path segments are re-encoded before parsing, so the characters urlencoding gives a
/// meaning to — `+`, a space, `&`, `=`, `%` — are the ones a lossy round trip would mangle.
#[tokio::test]
async fn a_path_segment_survives_the_re_encoding_exactly() {
    let (status, body) = probe("/named/a+b%20c%26d=e%25f").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"name":"a+b c&d=e%f"}"#);
}

#[tokio::test]
async fn a_path_segment_that_is_not_utf8_is_a_400_in_this_crates_error_shape() {
    let (status, body) = probe("/named/%FF").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.starts_with(r#"{"message":"#), "got {body}");
}
