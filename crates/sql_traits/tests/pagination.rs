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
