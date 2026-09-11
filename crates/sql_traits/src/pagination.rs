//! Pagination parameters and the response envelope.
//!
//! These types are the validated, post-HTTP shape: a limit that has already been checked
//! against a maximum, and an offset or cursor to start from. Nothing here enforces a limit —
//! see `axum_helpers::pagination`, which owns that policy, and the `ListRecordsPaginated`
//! docs below for what an implementation is trusted to do.

use ::async_trait::async_trait;
use ::sqlx::PgPool;

/// What a pagination mode contributes to a response.
///
/// Implemented by the validated parameter types, which is what lets a single route handler
/// serve every mode: the metadata travels back from the query alongside the rows, rather than
/// being assembled afterwards by code that cannot know a total or a next cursor.
pub trait PaginationParams {
    /// The metadata this mode's response carries.
    type Pagination;
}

/// A page of records plus its mode's metadata.
///
/// One envelope for every mode, with `M` varying, so a client reads `data` the same way no
/// matter which mode produced it. A paginated route always answers this, including for an
/// empty page — there is no shape that is sometimes an array and sometimes an object.
///
/// `Deserialize` as well as `Serialize`, so a Rust client or an integration test can parse a
/// response back into the same envelope the service sent. Derived bounds apply only where they
/// are used, so a record that is `Serialize`-only still serves this on the way out.
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
pub struct Page<T, M> {
    /// The records in this page.
    pub data: Vec<T>,
    /// How to interpret this page, and how to ask for the next one.
    pub pagination: M,
}

/// Offset-based parameters, already validated.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct OffsetParams {
    /// How many records to return. Already checked against the route's maximum.
    pub limit: u16,
    /// How many records to skip.
    pub offset: u32,
    /// Whether the caller asked for [`OffsetPagination::total`].
    pub include_total: bool,
}

/// Offset-based response metadata.
#[derive(Debug, Copy, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
pub struct OffsetPagination {
    /// The offset this page starts at.
    pub offset: u32,
    /// The limit this page was built with.
    pub limit: u16,
    /// How many records match in total, and `None` when the request did not ask. Filling it
    /// costs a count of the whole filtered set, which is why it is opt-in.
    pub total: Option<u32>,
}

/// Cursor-based parameters, already validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorParams<C> {
    /// How many records to return. Already checked against the route's maximum.
    pub limit: u16,
    /// Where to resume from, and `None` for the first page.
    pub cursor: Option<C>,
}

/// Cursor-based response metadata.
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
pub struct CursorPagination<C> {
    /// The limit this page was built with.
    pub limit: u16,
    /// The cursor for the next page, and `None` when this is the last one. Carries the
    /// cursor's own type, so an `i64` row id stays a JSON number.
    pub next: Option<C>,
}

impl PaginationParams for OffsetParams {
    type Pagination = OffsetPagination;
}

impl<C> PaginationParams for CursorParams<C> {
    type Pagination = CursorPagination<C>;
}

/// Retrieves one page of records of this type from the database.
///
/// The mode is the type parameter `P`: a record type offering both modes implements this
/// twice, at [`OffsetParams`] and at [`CursorParams`]. [`ListRecords`](crate::ListRecords) is
/// untouched and still returns every row.
///
/// # A cursor needs a total order
///
/// A cursor identifies a position in a sort, so the sort has to be total. `ORDER BY
/// created_at` with ties both skips and duplicates rows across pages, because a tied row can
/// fall on either side of the boundary between two queries. End the sort with a unique
/// tiebreaker — the primary key will do — and encode the whole sort key in the cursor, not
/// just its first column.
///
/// # Knowing whether another page exists
///
/// Fetch `limit + 1` rows, return the first `limit`, and set
/// [`CursorPagination::next`] from the extra one if it came back. Any other approach either
/// guesses or pays for a second count.
///
/// # `total` is opt-in
///
/// Fill [`OffsetPagination::total`] with `Some` if and only if
/// [`OffsetParams::include_total`] is set, and leave it `None` otherwise. A caller that asked
/// for it is paying for `count(*) OVER ()` over the whole filtered set, which a caller that
/// did not ask must not be charged for. The compiler cannot enforce that "if and only if", so
/// it is a contract.
///
/// # No limits are enforced here
///
/// `params.limit` has already been checked against a maximum by the time it arrives —
/// `axum_helpers` owns that policy, and a direct non-HTTP caller is trusted to pass something
/// sane. Do not second-guess it, and do not clamp it: a silently reduced page size is
/// indistinguishable from a short last page.
#[async_trait]
pub trait ListRecordsPaginated<P>: Sized
where
    P: PaginationParams + Send,
{
    async fn list_records_paginated(
        pool: &PgPool,
        params: P,
    ) -> Result<Page<Self, P::Pagination>, sqlx::Error>;
}

/// Retrieves one page of the records of this type matching the given filter.
///
/// Everything in [`ListRecordsPaginated`]'s documentation applies here too: the cursor's
/// total order, the `limit + 1` fetch, the opt-in total, and the absence of any limit
/// enforcement. The only difference is the filter, which narrows what is counted as well as
/// what is returned.
#[async_trait]
pub trait ListRecordsWherePaginated<T, P>: Sized
where
    T: Send,
    P: PaginationParams + Send,
{
    async fn list_records_where_paginated(
        pool: &PgPool,
        where_params: T,
        params: P,
    ) -> Result<Page<Self, P::Pagination>, sqlx::Error>;
}
