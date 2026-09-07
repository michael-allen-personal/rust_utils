//! `double_option` is what makes the three states of a partial update survive JSON.
//!
//! A plain derive on `Option<Option<T>>` collapses an explicit `null` into the same
//! `None` an absent key produces, which would silently turn "clear this column" into
//! "leave it alone". Every assertion here is about keeping those two apart.
//!
//! `serde` is reached through this crate's own re-export, the way a consumer following the
//! dependency convention would.

use sql_traits::serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(crate = "sql_traits::serde")]
struct WidgetUpdate {
    /// A non-nullable column: one `Option`, no helper needed.
    #[serde(default)]
    name: Option<String>,
    /// A nullable column: two `Option`s, and the helper to tell them apart.
    #[serde(default, deserialize_with = "sql_traits::double_option")]
    qty: Option<Option<i32>>,
}

fn parse(json: &str) -> WidgetUpdate {
    serde_json::from_str(json).expect("valid json")
}

#[test]
fn an_absent_nullable_field_is_none() {
    assert_eq!(parse("{}").qty, None);
}

#[test]
fn an_explicit_null_is_some_none() {
    assert_eq!(
        parse(r#"{"qty": null}"#).qty,
        Some(None),
        "an explicit null must stay distinguishable from an absent key"
    );
}

#[test]
fn a_value_is_some_some() {
    assert_eq!(parse(r#"{"qty": 7}"#).qty, Some(Some(7)));
}

#[test]
fn a_non_nullable_field_is_unaffected() {
    assert_eq!(parse("{}").name, None);
    assert_eq!(parse(r#"{"name": "cog"}"#).name, Some("cog".to_string()));
}
