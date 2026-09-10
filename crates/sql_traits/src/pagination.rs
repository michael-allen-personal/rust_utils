//! Pagination parameters and the response envelope.
//!
//! These types are the validated, post-HTTP shape: a limit that has already been checked
//! against a maximum, and an offset or cursor to start from. Nothing here enforces a limit —
//! see `axum_helpers::pagination`, which owns that policy, and the `ListRecordsPaginated`
//! docs below for what an implementation is trusted to do.

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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
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
