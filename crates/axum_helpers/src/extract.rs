use axum::{
    extract::{FromRequestParts, RawPathParams},
    http::request::Parts,
};
use serde::de::DeserializeOwned;

use crate::ApiError;

/// Extracts `T` from the route's path segments and the request's query string merged into one
/// set of keys, so one type can take `book_id` from `/books/{book_id}/reviews` and `min_rating`
/// from `?min_rating=4`. Every `*WhereRoute` reads its filter through this; it is public so a
/// hand-written handler can read a filter the same way.
///
/// ```ignore
/// async fn books_by_author(
///     State(pool): State<Pool<Sqlite>>,
///     PathAndQuery(filter): PathAndQuery<BookFilter>,
/// ) -> Response { /* … */ }
/// ```
///
/// Which source supplies a field is decided by the route it is mounted on, not by `T`: mounted
/// at `/authors/{author_id}/books`, a filter's `author_id` comes from the path; mounted at
/// `/books`, it has to come from `?author_id=`. A route with no segments, or a request with no
/// query string, contributes nothing.
///
/// # A key in both places is a `400`
///
/// The merged keys are deserialized once, so a key the path bound and the query repeats is a
/// duplicate field. The query can therefore never redirect a request away from the record the
/// path addresses — which matters wherever authorization was decided on the path.
///
/// # What `T` can be
///
/// - A struct with named fields. A bare `i64` or a tuple has no names to merge on; that is
///   `axum::extract::Path`'s job, and the reason this is not a drop-in for it.
/// - Not `#[serde(flatten)]`-ing anything it needs the duplicate check on: serde buffers
///   flattened keys and loses it.
/// - Not `#[serde(deny_unknown_fields)]` if it shares a query string with anything else, such
///   as a paginated route's `limit` and `cursor`, which it would then reject.
///
/// # Why the keys are re-encoded rather than handed over as pairs
///
/// The path segments are urlencoded ahead of the raw query string and the result is parsed by
/// `serde_urlencoded`, the parser behind axum's own `Query`. Only the segments are re-encoded —
/// usually one or two short values — and the round trip is exact, since axum hands them over
/// already percent-decoded. The alternative is a hand-written `Deserializer` over the pairs:
/// serde's own `MapDeserializer` passes a value through as a string and cannot parse `"1990"`
/// into an `i32`, and `serde_urlencoded` only accepts bytes. Writing that parser would let a
/// field parse differently here than from `?x=` under `Query`; re-encoding keeps them identical.
///
/// Rejects with [`ApiError`], so every failure is in this crate's `{"message": …}` shape: a
/// `400` for a field that is missing, unparseable or duplicated, or for a path segment that is
/// not valid UTF-8.
#[derive(Debug, Clone, Copy, Default)]
pub struct PathAndQuery<T>(pub T);

impl<T, S> FromRequestParts<S> for PathAndQuery<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let path = RawPathParams::from_request_parts(parts, state).await?;

        let mut merged = serde_urlencoded::to_string(path.iter().collect::<Vec<_>>())
            .expect("a sequence of string pairs always urlencodes");
        if let Some(query) = parts.uri.query().filter(|query| !query.is_empty()) {
            if !merged.is_empty() {
                merged.push('&');
            }
            merged.push_str(query);
        }

        Ok(Self(serde_urlencoded::from_str(&merged)?))
    }
}
