//! The query-string types in isolation: what a URL deserializes into, what is filled in when
//! it says nothing, what is rejected, and what reaches the SQL layer.
//!
//! `Query::try_from_uri` drives these directly, so the assertions are about the types rather
//! than about a handler. `route_responses.rs` covers the handler.
//!
//! Everything is reached through `axum_helpers`' re-exports, never declared directly.

use axum_helpers::axum::extract::Query;
use axum_helpers::axum::http::Uri;
use axum_helpers::sql_traits::{CursorParams, OffsetParams};
use axum_helpers::{CursorParamsQuery, OffsetParamsQuery, PaginationQuery};

/// A deliberately tight policy: a maximum *below* the crate-wide default of 50, which is the
/// case that rules out defaulting at the serde level.
type Offset = OffsetParamsQuery<10, 25>;

/// A cursor type with no `Default` of its own, so a derived `Default` on the query type would
/// not compile. Deserialized by hand to keep this file free of a `serde` derive dependency
/// question.
#[derive(Debug, PartialEq)]
struct Token(i64);

impl<'de> axum_helpers::serde::Deserialize<'de> for Token {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: axum_helpers::serde::Deserializer<'de>,
    {
        <i64 as axum_helpers::serde::Deserialize>::deserialize(deserializer).map(Token)
    }
}

fn offset(uri: &str) -> Offset {
    let uri: Uri = uri.parse().expect("a valid uri");
    let Query(params) = Query::try_from_uri(&uri).expect("the query string deserializes");
    params
}

fn cursor(uri: &str) -> CursorParamsQuery<Token, 10, 25> {
    let uri: Uri = uri.parse().expect("a valid uri");
    let Query(params) = Query::try_from_uri(&uri).expect("the query string deserializes");
    params
}

/// A bare request is the common case and must work. The filled limit comes from *this type's*
/// parameters, not from any crate-wide number, which is why a maximum of 25 does not reject it.
#[test]
fn a_request_naming_nothing_takes_this_types_own_default() {
    let params = offset("/widgets");

    assert_eq!(params.limit, 10);
    assert_eq!(params.offset, 0);
    assert!(!params.include_total);
    assert_eq!(params.validate(), Ok(()));
}

#[test]
fn a_partial_request_keeps_the_default_for_what_it_did_not_name() {
    let params = offset("/widgets?offset=40");

    assert_eq!((params.limit, params.offset), (10, 40));
}

#[test]
fn a_named_limit_wins_over_the_default() {
    let params = offset("/widgets?limit=5&include_total=true");

    assert_eq!(
        (params.limit, params.offset, params.include_total),
        (5, 0, true)
    );
}

/// An explicit zero must survive deserialization rather than being defaulted away, because
/// rejecting it is the whole point.
#[test]
fn an_explicit_zero_survives_and_is_then_rejected() {
    let params = offset("/widgets?limit=0");

    assert_eq!(params.limit, 0);
    assert_eq!(
        params.validate(),
        Err(axum_helpers::RequestError::InvalidPaginationLimit {
            requested: 0,
            max: 25
        })
    );
}

#[test]
fn the_maximum_is_inclusive() {
    assert_eq!(offset("/widgets?limit=25").validate(), Ok(()));
    assert_eq!(
        offset("/widgets?limit=26").validate(),
        Err(axum_helpers::RequestError::InvalidPaginationLimit {
            requested: 26,
            max: 25
        })
    );
}

#[test]
fn an_unparseable_limit_is_a_deserialization_failure_not_a_default() {
    let uri: Uri = "/widgets?limit=abc".parse().expect("a valid uri");
    let result: Result<Query<Offset>, _> = Query::try_from_uri(&uri);

    assert!(
        result.is_err(),
        "a non-numeric limit must be rejected rather than silently defaulted"
    );
}

/// The policy is readable without counting generic positions.
#[test]
fn the_policy_is_readable_as_associated_consts() {
    assert_eq!(Offset::DEFAULT_LIMIT, 10);
    assert_eq!(Offset::MAX_LIMIT, 25);
}

/// `Default` is the one place the default limit is stated, and the container's serde attribute
/// points at it, so a constructed value and a bare request agree.
#[test]
fn default_and_a_bare_request_agree() {
    assert_eq!(Offset::default(), offset("/widgets"));
}

#[test]
fn an_offset_query_converts_into_the_validated_params() {
    let params: OffsetParams = offset("/widgets?limit=5&offset=40&include_total=true").into();

    assert_eq!(
        params,
        OffsetParams {
            limit: 5,
            offset: 40,
            include_total: true
        }
    );
}

#[test]
fn a_request_with_no_cursor_is_the_first_page() {
    let params = cursor("/events");

    assert_eq!(params.limit, 10);
    assert_eq!(params.cursor, None);
}

/// The cursor type needs no `Default` of its own, which a derived `Default` on the query type
/// would have demanded.
#[test]
fn a_cursor_query_converts_into_the_validated_params() {
    let params: CursorParams<Token> = cursor("/events?limit=5&cursor=42").into();

    assert_eq!(params.limit, 5);
    assert_eq!(params.cursor, Some(Token(42)));
}
