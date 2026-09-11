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

#[derive(sql_traits::serde::Serialize)]
#[serde(crate = "sql_traits::serde")]
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
// Implementing them is the assertion. This crate has no async runtime in its
// dev-dependencies and does not need one: what can go wrong here is a signature that cannot
// be satisfied from outside, and that is a compile error, not a test failure.

use sql_traits::async_trait::async_trait;
use sql_traits::sqlx::{self, PgPool};
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
        _pool: &PgPool,
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
        _pool: &PgPool,
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
        _pool: &PgPool,
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
