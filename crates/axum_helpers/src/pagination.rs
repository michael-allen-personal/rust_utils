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
/// request may ask for. Write them in that order — a transposed pair is a compile error,
/// since `DEFAULT_LIMIT` must not exceed `MAX_LIMIT`.
///
/// Every missing field comes from [`Default`], so `limit` needs no `Option`: a request naming
/// no limit deserializes straight to `DEFAULT_LIMIT`. The container attribute has to name that
/// path rather than being a bare `#[serde(default)]`, which panics `serde_derive` on a
/// const-generic struct.
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default = "OffsetParamsQuery::<DEFAULT_LIMIT, MAX_LIMIT>::default")]
pub struct OffsetParamsQuery<const DEFAULT_LIMIT: u16 = 50, const MAX_LIMIT: u16 = 200> {
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
/// `CursorParamsQuery<i64>` and allocates nothing, while one needing an opaque composite
/// cursor writes `CursorParamsQuery<String>`. Cursor opacity is the implementor's decision.
///
/// The const parameters mean exactly what they do on [`OffsetParamsQuery`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default = "CursorParamsQuery::<C, DEFAULT_LIMIT, MAX_LIMIT>::default")]
pub struct CursorParamsQuery<C, const DEFAULT_LIMIT: u16 = 50, const MAX_LIMIT: u16 = 200> {
    /// How many records to return.
    pub limit: u16,
    /// Where to resume from, and `None` for the first page.
    pub cursor: Option<C>,
}

impl<const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16> OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT> {
    /// The limit a request that names none resolves to.
    pub const DEFAULT_LIMIT: u16 = DEFAULT_LIMIT;
    /// The largest limit a request may ask for.
    pub const MAX_LIMIT: u16 = MAX_LIMIT;

    /// Catches an incoherent policy at compile time. The parameters are positional, so the
    /// failure worth catching is a transposed pair: every genuine transposition makes the
    /// default exceed the maximum, except `DEFAULT_LIMIT == MAX_LIMIT`, where transposing
    /// changes nothing.
    const POLICY_IS_COHERENT: () = {
        assert!(MAX_LIMIT >= 1, "MAX_LIMIT must be at least 1");
        assert!(
            DEFAULT_LIMIT <= MAX_LIMIT,
            "DEFAULT_LIMIT exceeds MAX_LIMIT — are the parameters transposed?"
        );
    };
}

impl<C, const DEFAULT_LIMIT: u16, const MAX_LIMIT: u16>
    CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>
{
    /// The limit a request that names none resolves to.
    pub const DEFAULT_LIMIT: u16 = DEFAULT_LIMIT;
    /// The largest limit a request may ask for.
    pub const MAX_LIMIT: u16 = MAX_LIMIT;

    /// See [`OffsetParamsQuery::POLICY_IS_COHERENT`]; the limit parameters mean the same here.
    const POLICY_IS_COHERENT: () = {
        assert!(MAX_LIMIT >= 1, "MAX_LIMIT must be at least 1");
        assert!(
            DEFAULT_LIMIT <= MAX_LIMIT,
            "DEFAULT_LIMIT exceeds MAX_LIMIT — are the parameters transposed?"
        );
    };
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
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(RequestError::InvalidPaginationLimit {
                requested: self.limit,
                max: MAX_LIMIT,
            });
        }
        Ok(())
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
        if self.limit == 0 || self.limit > MAX_LIMIT {
            return Err(RequestError::InvalidPaginationLimit {
                requested: self.limit,
                max: MAX_LIMIT,
            });
        }
        Ok(())
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
