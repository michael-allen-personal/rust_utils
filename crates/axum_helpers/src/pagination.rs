//! The query-string side of pagination, and the limit policy.
//!
//! `sql_traits` holds the validated parameters and the response envelope; these are what a URL
//! deserializes into, and the conversion between the two is where a limit is checked and a
//! default filled in. The policy travels as const generic parameters on the query type, so the
//! conversion into the validated type is an infallible, total `From` with nothing left to
//! decide, and `validate` needs no arguments.

use serde::de::DeserializeOwned;
use sql_traits::{CursorParams, OffsetParams, PaginationParams};

use crate::RequestError;

/// A query type paired with the validated parameters it resolves to.
///
/// The same mutual shape as `sql_traits`' `RequestBody`/`HasRequestBody`: the supertrait
/// `Into<Self::Params>` is what lets one route handler body serve every mode, and it needs no
/// extra bound at each use site.
pub trait PaginationQuery: Sized + Into<Self::Params> + DeserializeOwned + Send + 'static {
    /// The validated parameters this query resolves to.
    type Params: PaginationParams + Send;

    /// Rejects a limit outside the range this type's parameters allow.
    ///
    /// Takes no arguments because the policy is in the type.
    fn validate(&self) -> Result<(), RequestError>;
}

/// Offset-based pagination parameters as they arrive on a URL.
///
/// `DEFAULT_LIMIT` fills in for a request that names no limit; `MAX_LIMIT` is the largest a
/// request may ask for. Write them in that order — a transposed pair fails to build, since
/// `DEFAULT_LIMIT` must not exceed `MAX_LIMIT`. That failure is a post-monomorphization error,
/// so it is `cargo build` and `cargo test` that surface it; `cargo check` (and an editor
/// running it) passes a transposed pair.
///
/// **Neither parameter has a default, so both are always written.** A default on `MAX_LIMIT`
/// would make partial specification legal and silently wrong: `OffsetParamsQuery<10>` reads as
/// "cap this route at 10" and would mean a default of 10 with the crate's maximum of 200 — an
/// intended maximum paired with the wrong default, serving twenty times the page size meant,
/// with nothing incoherent for the policy assertion to catch. The bare form is available as
/// [`DefaultOffsetParamsQuery`], which names the policy it carries.
///
/// Every missing field comes from [`Default`], so `limit` needs no `Option`: a request naming
/// no limit deserializes straight to `DEFAULT_LIMIT`. The container attribute has to name that
/// path rather than being a bare `#[serde(default)]`, which panics `serde_derive` on a
/// const-generic struct.
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default = "OffsetParamsQuery::<DEFAULT_LIMIT, MAX_LIMIT>::default")]
pub struct OffsetParamsQuery<const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> {
    /// How many records to return.
    pub limit: u16,
    /// How many records to skip.
    pub offset: u32,
    /// Whether to count every matching row and report the total.
    pub include_total: bool,
}

/// Cursor-based pagination parameters as they arrive on a URL.
///
/// `C` is the cursor's own type, so an implementor whose cursor is a row id writes
/// `CursorParamsQuery<i64, 50, 200>` and allocates nothing, while one needing an opaque
/// composite cursor writes `CursorParamsQuery<String, 50, 200>`. Cursor opacity is the
/// implementor's decision.
///
/// `C` must be `Serialize` as well as `Deserialize`, even though only the latter is bounded
/// here. The route traits bound the mode's metadata `Serialize`, and a cursor's metadata is
/// `CursorPagination<C>` — `next` goes back out as the type that came in, so a cursor type that
/// cannot serialize fails at the route-trait impl rather than here.
///
/// # A malformed cursor is the cursor type's own `400`
///
/// `ListRecordsPaginated` returns `Result<_, sqlx::Error>` and every `sqlx::Error` becomes a
/// `500`, so an implementation handed client-supplied garbage has no channel for "this cursor is
/// not a cursor". Decode inside `C`'s own `Deserialize` instead: make the opaque cursor a type
/// whose `Deserialize` does the base64-and-parse and fails on anything else, rather than a bare
/// `String` the implementation decodes later. A cursor that does not decode is then a
/// `QueryRejection`, which reaches the client as this crate's `400` before the handler runs —
/// the same path an unparseable `?limit=abc` takes.
///
/// The const parameters mean exactly what they do on [`OffsetParamsQuery`], defaults included:
/// there are none, so both are always written, for the reason given there. The bare form is
/// available as [`DefaultCursorParamsQuery`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default = "CursorParamsQuery::<C, DEFAULT_LIMIT, MAX_LIMIT>::default")]
pub struct CursorParamsQuery<C, const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> {
    /// How many records to return.
    pub limit: u16,
    /// Where to resume from, and `None` for the first page.
    pub cursor: Option<C>,
}

/// [`OffsetParamsQuery`] with this crate's default policy: 50 records per page, 200 at most.
///
/// The query types take no parameter defaults, so this alias is how the bare form is written.
/// A route wanting a different policy names both numbers itself —
/// `OffsetParamsQuery<20, 100>` — rather than overriding one of them.
pub type DefaultOffsetParamsQuery = OffsetParamsQuery<50, 200>;

/// [`CursorParamsQuery`] with this crate's default policy: 50 records per page, 200 at most.
///
/// The cursor type `C` stays a parameter, since no default could be right for it; only the limit
/// policy is fixed. A route wanting a different policy names both numbers itself —
/// `CursorParamsQuery<i64, 20, 100>`.
pub type DefaultCursorParamsQuery<C> = CursorParamsQuery<C, 50, 200>;

/// Catches an incoherent limit policy at build time. The const parameters are positional, so
/// the failure worth catching is a transposed pair: every genuine transposition makes the default
/// exceed the maximum, except `DEFAULT_LIMIT == MAX_LIMIT`, where transposing changes nothing.
/// A zero default is the other incoherence, and the one `validate` would otherwise turn into a
/// permanent `400` for every parameter-less request.
///
/// Shared by both query types rather than written twice, because the check concerns only the limit
/// parameters, which both carry identically. Reached through each type's `POLICY_IS_COHERENT`,
/// whose evaluation is what makes a bad pair fail to build. Post-monomorphization, so it takes
/// `cargo build` or `cargo test` — `cargo check` never evaluates the constant.
const fn assert_policy_coherent(default_limit: u16, max_limit: u16) {
    assert!(max_limit >= 1, "MAX_LIMIT must be at least 1");
    assert!(default_limit >= 1, "DEFAULT_LIMIT must be at least 1");
    assert!(
        default_limit <= max_limit,
        "DEFAULT_LIMIT exceeds MAX_LIMIT — are the parameters transposed?"
    );
}

/// The limit check both query types perform. Shared for the same reason as
/// [`assert_policy_coherent`]: it reads only the limit, so neither wire shape's own fields enter
/// into it, and one copy means one place to change the rule.
fn validate_limit(limit: u16, max_limit: u16) -> Result<(), RequestError> {
    if limit == 0 || limit > max_limit {
        return Err(RequestError::InvalidPaginationLimit {
            requested: limit,
            max: max_limit,
        });
    }
    Ok(())
}

impl<const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT> {
    /// The limit a request that names none resolves to.
    pub const DEFAULT_LIMIT: u16 = DEFAULT_LIMIT;
    /// The largest limit a request may ask for.
    pub const MAX_LIMIT: u16 = MAX_LIMIT;

    /// Evaluated by [`PaginationQuery::validate`], which is what makes a transposed or zero
    /// parameter pair fail to build rather than be a runtime surprise. Because the evaluation is
    /// post-monomorphization it takes a real build — `cargo build` or `cargo test` reports it,
    /// `cargo check` and an editor running it do not.
    const POLICY_IS_COHERENT: () = assert_policy_coherent(DEFAULT_LIMIT, MAX_LIMIT);
}

impl<C, const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16>
    CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>
{
    /// The limit a request that names none resolves to.
    pub const DEFAULT_LIMIT: u16 = DEFAULT_LIMIT;
    /// The largest limit a request may ask for.
    pub const MAX_LIMIT: u16 = MAX_LIMIT;

    /// See [`OffsetParamsQuery::POLICY_IS_COHERENT`]; the limit parameters mean the same here,
    /// and so does needing a real build rather than a `cargo check` to report a bad pair.
    const POLICY_IS_COHERENT: () = assert_policy_coherent(DEFAULT_LIMIT, MAX_LIMIT);
}

/// The one place the default limit is stated. The container's `serde` attribute points here,
/// so a bare request and a constructed value agree.
impl<const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> Default
    for OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>
{
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            offset: 0,
            include_total: false,
        }
    }
}

/// Hand-written rather than derived: `#[derive(Default)]` adds a `C: Default` bound for every
/// type parameter without checking whether a field needs one, and no cursor type has reason to
/// satisfy it. `Option<C>` defaults to `None` for every `C`.
impl<C, const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> Default
    for CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>
{
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            cursor: None,
        }
    }
}

impl<const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> PaginationQuery
    for OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>
{
    type Params = OffsetParams;

    fn validate(&self) -> Result<(), RequestError> {
        let () = Self::POLICY_IS_COHERENT;
        validate_limit(self.limit, MAX_LIMIT)
    }
}

impl<C, const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> PaginationQuery
    for CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>
where
    C: DeserializeOwned + Send + 'static,
{
    type Params = CursorParams<C>;

    fn validate(&self) -> Result<(), RequestError> {
        let () = Self::POLICY_IS_COHERENT;
        validate_limit(self.limit, MAX_LIMIT)
    }
}

/// A pure field move. The default is stated in [`Default`] and the maximum is checked in
/// [`PaginationQuery::validate`], so there is no policy left in here to drift.
///
/// Legal in this crate despite `OffsetParams` being foreign: the orphan rules allow
/// `impl From<LocalType> for ForeignType` because the local type appears in the trait's
/// parameter position.
impl<const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16>
    From<OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>> for OffsetParams
{
    fn from(query: OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>) -> Self {
        OffsetParams {
            limit: query.limit,
            offset: query.offset,
            include_total: query.include_total,
        }
    }
}

/// A pure field move, for the same reasons as the offset conversion.
impl<C, const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16>
    From<CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>> for CursorParams<C>
{
    fn from(query: CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>) -> Self {
        CursorParams {
            limit: query.limit,
            cursor: query.cursor,
        }
    }
}
