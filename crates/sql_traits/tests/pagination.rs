//! The pagination envelope's wire shape.
//!
//! These types are plain data, and the thing worth pinning down is the JSON a consumer's
//! client will parse: one outer shape for every mode, with the mode-specific metadata
//! nested under `pagination`. No axum and no `Router` here — `axum_helpers`'
//! `route_responses.rs` covers behaviour, this covers format.
//!
//! `serde` is reached through this crate's own re-export, never declared directly, so the
//! test also proves the re-export surface is enough to build these types from outside.

use sql_traits::{CursorPagination, OffsetPagination, Page};

#[derive(sql_traits::serde::Serialize, sql_traits::Database)]
#[serde(crate = "sql_traits::serde")]
#[sql_traits(database = Sqlite)]
struct Widget {
    id: i64,
}

fn widgets() -> Vec<Widget> {
    vec![Widget { id: 1 }, Widget { id: 2 }]
}

#[test]
fn an_offset_page_is_data_plus_a_nested_pagination_object() {
    let page = Page {
        data: widgets(),
        pagination: OffsetPagination {
            offset: 40,
            limit: 20,
            total: Some(1234),
        },
    };

    let json = serde_json::to_string(&page).expect("the envelope is serializable");

    assert_eq!(
        json,
        r#"{"data":[{"id":1},{"id":2}],"pagination":{"offset":40,"limit":20,"total":1234}}"#
    );
}

/// `total` costs a count of the whole filtered set, so it is only filled when the request
/// asked for it. An unasked-for total is `null`, not a guess and not an absent key.
#[test]
fn an_unrequested_total_serializes_as_null() {
    let page = Page {
        data: widgets(),
        pagination: OffsetPagination {
            offset: 0,
            limit: 20,
            total: None,
        },
    };

    let json = serde_json::to_string(&page).expect("the envelope is serializable");

    assert!(
        json.contains(r#""total":null"#),
        "an unrequested total must be an explicit null, got {json}"
    );
}

/// The point of one envelope for both modes: a client reads `data` the same way regardless,
/// and only the contents of `pagination` differ.
#[test]
fn a_cursor_page_uses_the_same_outer_shape() {
    let page = Page {
        data: widgets(),
        pagination: CursorPagination {
            limit: 20,
            next: Some(4_815_162_342_i64),
        },
    };

    let json = serde_json::to_string(&page).expect("the envelope is serializable");

    assert_eq!(
        json,
        r#"{"data":[{"id":1},{"id":2}],"pagination":{"limit":20,"next":4815162342}}"#
    );
}

/// `next` carries the cursor's own type rather than being stringified, which is what lets an
/// implementor whose cursor is a row id pay nothing for it in either direction.
#[test]
fn a_cursor_round_trips_as_its_own_type_not_as_a_string() {
    let page: Page<Widget, CursorPagination<i64>> = Page {
        data: Vec::new(),
        pagination: CursorPagination {
            limit: 20,
            next: Some(42),
        },
    };

    let json = serde_json::to_string(&page).expect("the envelope is serializable");

    assert!(
        json.contains(r#""next":42"#),
        "an i64 cursor must serialize as a number, got {json}"
    );
}

#[test]
fn the_last_page_has_no_next_cursor() {
    let page: Page<Widget, CursorPagination<i64>> = Page {
        data: Vec::new(),
        pagination: CursorPagination {
            limit: 20,
            next: None,
        },
    };

    let json = serde_json::to_string(&page).expect("the envelope is serializable");

    assert_eq!(json, r#"{"data":[],"pagination":{"limit":20,"next":null}}"#);
}

// --- The traits, from a consumer's vantage point -------------------------------------
//
// The `Widget` fixtures below are compile-only: implementing the traits is the assertion,
// so what can go wrong is a signature that cannot be satisfied from outside, and that is a
// compile error, not a test failure. `Entry`, further down, is the opposite: it awaits real
// queries against a real in-memory SQLite database, which is what this crate's `tokio` and
// `runtime-tokio` dev-dependencies are for.

use sql_traits::async_trait::async_trait;
use sql_traits::sqlx::{self, Pool, Sqlite};
use sql_traits::{CursorParams, ListRecordsPaginated, ListRecordsWherePaginated, OffsetParams};

/// The filter a `*Where` implementation takes. Its contents do not matter here, which is why
/// the field is never read.
#[allow(dead_code)]
struct WidgetFilter {
    owner_id: i64,
}

#[async_trait]
impl ListRecordsPaginated<OffsetParams> for Widget {
    async fn list_records_paginated(
        _pool: &Pool<Sqlite>,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        Ok(Page {
            data: Vec::new(),
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: params.include_total.then_some(0),
            },
        })
    }
}

/// The same record type in the other mode, which is what makes the mode a type parameter
/// rather than a second trait.
#[async_trait]
impl ListRecordsPaginated<CursorParams<i64>> for Widget {
    async fn list_records_paginated(
        _pool: &Pool<Sqlite>,
        params: CursorParams<i64>,
    ) -> Result<Page<Self, CursorPagination<i64>>, sqlx::Error> {
        Ok(Page {
            data: Vec::new(),
            pagination: CursorPagination {
                limit: params.limit,
                next: None,
            },
        })
    }
}

#[async_trait]
impl ListRecordsWherePaginated<WidgetFilter, OffsetParams> for Widget {
    async fn list_records_where_paginated(
        _pool: &Pool<Sqlite>,
        _where_params: WidgetFilter,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        Ok(Page {
            data: Vec::new(),
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: None,
            },
        })
    }
}

/// A where-clause assertion: this function is never called, and it compiles only if every
/// impl above actually satisfies the trait it names.
#[allow(dead_code)]
fn the_paginated_traits_are_implementable_from_outside()
where
    Widget: ListRecordsPaginated<OffsetParams>
        + ListRecordsPaginated<CursorParams<i64>>
        + ListRecordsWherePaginated<WidgetFilter, OffsetParams>,
{
}

// --- Real paginated queries ------------------------------------------------------------
//
// `Widget` above proves the traits are implementable from outside the crate at all — a
// compile-time assertion with no rows and no runtime. `Entry` below proves the contract
// `ListRecordsPaginated`'s documentation states and no fake impl can be held to: a cursor
// needs a total order, another page is detected by fetching `limit + 1`, and `total` is
// filled if and only if it was asked for. These tests run real queries against a real
// in-memory SQLite database.

#[derive(sql_traits::Database)]
#[sql_traits(database = Sqlite)]
struct Entry {
    id: i64,
}

async fn seeded_pool(rows: i64) -> Pool<Sqlite> {
    let pool = Pool::<Sqlite>::connect("sqlite::memory:")
        .await
        .expect("an in-memory database");
    sqlx::query("CREATE TABLE entry (id INTEGER PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("the schema");
    for id in 1..=rows {
        sqlx::query("INSERT INTO entry (id) VALUES (?)")
            .bind(id)
            .execute(&pool)
            .await
            .expect("a seeded row");
    }
    pool
}

#[async_trait]
impl ListRecordsPaginated<OffsetParams> for Entry {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (i64,)>("SELECT id FROM entry ORDER BY id LIMIT ? OFFSET ?")
            .bind(params.limit as i64)
            .bind(params.offset as i64)
            .fetch_all(pool)
            .await?;

        // Filled if and only if the caller asked, because it costs a count of the whole set.
        let total = if params.include_total {
            let (count,) = sqlx::query_as::<_, (i64,)>("SELECT count(*) FROM entry")
                .fetch_one(pool)
                .await?;
            Some(count as u32)
        } else {
            None
        };

        Ok(Page {
            data: rows.into_iter().map(|(id,)| Entry { id }).collect(),
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total,
            },
        })
    }
}

#[async_trait]
impl ListRecordsPaginated<CursorParams<i64>> for Entry {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: CursorParams<i64>,
    ) -> Result<Page<Self, CursorPagination<i64>>, sqlx::Error> {
        // Fetch one more than asked for: the extra row is how another page is detected
        // without paying for a second count. `id` is the primary key, so the sort is
        // total and no row can fall on both sides of a page boundary.
        let mut rows =
            sqlx::query_as::<_, (i64,)>("SELECT id FROM entry WHERE id > ? ORDER BY id LIMIT ?")
                .bind(params.cursor.unwrap_or(0))
                .bind(params.limit as i64 + 1)
                .fetch_all(pool)
                .await?;

        let next = if rows.len() > params.limit as usize {
            rows.pop();
            rows.last().map(|(id,)| *id)
        } else {
            None
        };

        Ok(Page {
            data: rows.into_iter().map(|(id,)| Entry { id }).collect(),
            pagination: CursorPagination {
                limit: params.limit,
                next,
            },
        })
    }
}

#[tokio::test]
async fn an_unrequested_total_is_not_counted() {
    let pool = seeded_pool(10).await;
    let params = OffsetParams {
        limit: 3,
        offset: 0,
        include_total: false,
    };

    let page = <Entry as ListRecordsPaginated<OffsetParams>>::list_records_paginated(&pool, params)
        .await
        .expect("the page");

    assert_eq!(page.data.len(), 3);
    assert_eq!(
        page.pagination.total, None,
        "a total nobody asked for must not be filled"
    );
}

#[tokio::test]
async fn a_requested_total_counts_the_whole_set_not_the_page() {
    let pool = seeded_pool(10).await;
    let params = OffsetParams {
        limit: 3,
        offset: 0,
        include_total: true,
    };

    let page = <Entry as ListRecordsPaginated<OffsetParams>>::list_records_paginated(&pool, params)
        .await
        .expect("the page");

    assert_eq!(page.data.len(), 3, "the page is still a page");
    assert_eq!(page.pagination.total, Some(10));
}

/// A page that is not full is the last page, and must say so. Reporting a next cursor here
/// costs the caller an extra round trip to discover an empty page.
#[tokio::test]
async fn the_last_page_reports_no_next_cursor() {
    let pool = seeded_pool(4).await;
    let params = CursorParams {
        limit: 10,
        cursor: None,
    };

    let page =
        <Entry as ListRecordsPaginated<CursorParams<i64>>>::list_records_paginated(&pool, params)
            .await
            .expect("the page");

    assert_eq!(page.data.len(), 4);
    assert_eq!(
        page.pagination.next, None,
        "there is no page after the last one"
    );
}

/// The promise that makes cursor pagination worth having: walking every page visits each
/// row exactly once, with nothing skipped and nothing repeated.
#[tokio::test]
async fn walking_the_cursor_visits_every_row_exactly_once() {
    let pool = seeded_pool(10).await;
    let mut seen = Vec::new();
    let mut cursor = None;

    loop {
        let params = CursorParams { limit: 3, cursor };
        let page = <Entry as ListRecordsPaginated<CursorParams<i64>>>::list_records_paginated(
            &pool, params,
        )
        .await
        .expect("the page");
        seen.extend(page.data.iter().map(|entry| entry.id));
        match page.pagination.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    assert_eq!(seen, (1..=10).collect::<Vec<i64>>());
}

/// A page that is exactly full is still the last page when nothing follows it. This is the
/// boundary `limit + 1` exists for: fetching one extra row is the only way to tell a full
/// final page from a full page with more behind it, and an off-by-one in that check silently
/// drops a row as well as inventing a cursor.
#[tokio::test]
async fn a_full_final_page_reports_no_next_cursor() {
    let pool = seeded_pool(3).await;
    let params = CursorParams {
        limit: 3,
        cursor: None,
    };

    let page =
        <Entry as ListRecordsPaginated<CursorParams<i64>>>::list_records_paginated(&pool, params)
            .await
            .expect("the page");

    assert_eq!(page.data.len(), 3, "a full final page must not lose a row");
    assert_eq!(
        page.pagination.next, None,
        "nothing follows a full final page"
    );
}
