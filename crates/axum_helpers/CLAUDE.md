# axum_helpers

One axum route handler per `sql_traits` database trait, as a trait with a provided method,
plus the error type that maps failures onto status codes. A consumer implements the SQL
trait, writes `impl GetRecordRoute for Widget {}` (or derives it), and mounts
`Widget::get_record_route`.

Handlers are provided methods rather than free functions so a type opts in by naming the
trait, and so the supertrait list states the requirements — a missing `GetRecord` impl is an
error on the route trait, not a mystery at the mount site.

Every dependency whose types leak into these signatures is re-exported, `sql_traits`
included; see the root `CLAUDE.md`. All four test crates reach `axum`, `serde`, `sqlx` and
`sql_traits` *only* through those re-exports, so the re-export surface being sufficient to
write a handler is itself under test.

## Trait ↔ route ↔ status code

| SQL trait | Route trait | Success | Empty result |
|---|---|---|---|
| `GetRecord` | `GetRecordRoute` | `200` + JSON | `404` |
| `GetRecordWhere<T>` | `GetRecordWhereRoute<T>` | `200` + JSON | `404` |
| `GetLatestRecord` | `GetLatestRoute` | `200` + JSON | `204` |
| `ListRecords` / `ListRecordsWhere<T>` | `ListRecordsRoute` / `ListRecordsWhereRoute<T>` | `200` + array | — |
| `ListRecordsPaginated<OffsetParams>` | `ListRecordsPaginatedRoute<OffsetParamsQuery<..>>` | `200` + envelope | — |
| `ListRecordsPaginated<CursorParams<C>>` | `ListRecordsPaginatedRoute<CursorParamsQuery<C, ..>>` | `200` + envelope | — |
| `ListRecordsWherePaginated<T, P>` | `ListRecordsWherePaginatedRoute<T, Q>` | `200` + envelope | — |
| `InsertRecord` / `BulkInsertRecords` | `CreateRoute` / `BulkCreateRoute` | `201` + JSON | — |
| `ReplaceRecord` | `ReplaceRoute` | `200` + JSON | `404` |
| `UpdateRecord` | `UpdateRoute` | `200` + JSON | `404`, or `400` if the body is empty |
| `DeleteRecord` | `DeleteRoute` | `204` | — |
| `DeleteRecordsWhere<T>` | `DeleteRecordsWhereRoute<T>` | `200` + JSON | — |

`GetLatestRoute` is the one `204`: an empty table is not a missing resource, whereas a
caller who addressed a specific record and got nothing has hit a `404`. The three fetch-one
handlers share `optional_record_response`, which takes the not-found behaviour as its only
parameter — keep that the single place the decision is made.

`DeleteRoute` discards its return value; `DeleteRecordsWhereRoute` serializes it, so a caller
can see what the delete actually matched.

## Two things route traits enforce that are easy to lose

**`DeserializeOwned` on anything extracted from the path.** Without it a type can satisfy the
route trait and only fail at `Router::route`, with an opaque `Handler` error that never
mentions `Deserialize`. v0.7.0 tightened these bounds for exactly that reason. Any new
path-extracting trait needs the same bound.

**An empty update body is a `400`, checked before the pool is touched.** Left to the
implementation, a dynamically built `UPDATE` with an empty `SET` list is a SQL syntax error
and a `500`. Checked here it is one branch for every implementor at once, and it runs before
the key lookup, so an empty body is a `400` whether or not the row exists.

## Composite primary keys bind by name

A composite `PrimaryKey` is the `{Name}PrimaryKey` struct `macros` generates, and
`axum::extract::Path` fills a struct by field name. Route segments must be *named* after the
key's fields; the order a route declares them in does not matter, extra segments are ignored,
and a key field no segment names is a `400` from the extractor before the handler runs.

This is why the generated key type is a struct and not a tuple — see `crates/macros/CLAUDE.md`.

## Pagination is a separate route trait, never a mode of `ListRecordsRoute`

`ListRecordsRoute` answers a bare array forever; `ListRecordsPaginatedRoute` answers an envelope
always — `{"data": [...], "pagination": {...}}`, empty pages included, never a `204`. A response
is never sometimes an array and sometimes an object, and nothing branches on whether pagination
parameters were present: a type opts in by naming the trait, which is the same reason these are
traits with provided methods rather than free functions.

A `Link` header was the alternative considered, and was rejected. It cannot carry a total, so
offset mode would need a second non-standard header regardless; it obliges every client to parse
RFC 8288 before it can follow a page; and building a correct absolute URL needs `OriginalUri`
plus the mount prefix and any proxy rewriting, none of which a provided method reliably knows.
The envelope is also the only mechanism that serves both modes with one shape.

A record type offering both modes has two impls of the same provided method, so its mount site
needs a turbofish:
`<Gadget as ListRecordsPaginatedRoute<DefaultOffsetParamsQuery>>::list_records_paginated_route`.
That is the existing situation for the `*Where` family rather than a new problem, but it is what
offering both modes on one type costs at the router. `tests/route_traits.rs` mounts such a type,
so the disambiguation itself is under test.

## The limit policy lives in the query type

`OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>` carries its policy as const generic parameters, so
a mount site states it where a reader will see it (`impl ListRecordsPaginatedRoute<OffsetParamsQuery<20, 100>> for Invoice {}`)
and the conversion into `sql_traits`' validated types is an infallible, total `From`. The default
is stated once, in `Default`; the maximum is checked by `validate`, which needs no arguments
because it reads the type. Nothing restates either.

**Neither parameter has a default.** Both are always written, so the bare `OffsetParamsQuery` is
spelled `DefaultOffsetParamsQuery` (50 / 200) and the cursor one `DefaultCursorParamsQuery<C>`.
Parameter defaults made *partial* specification legal and silently wrong: `OffsetParamsQuery<10>`
reads as "cap this route at 10" and meant a default of 10 with the inherited maximum of 200 — a
route serving twenty times the intended page size, with nothing incoherent for
`POLICY_IS_COHERENT` to catch. Same failure shape as the positional composite key, and rejected
for the same reason.

The parameters are positional, so a transposed pair is the failure worth catching, and
`POLICY_IS_COHERENT` makes it fail to build. Every genuine transposition trips it, since it
makes the default exceed the maximum — the sole exception is `DEFAULT == MAX`, where transposing
changes nothing. A zero default trips the companion assertion, which exists because `validate`
rejects `limit == 0`: `<0, 100>` would otherwise make every parameter-less request a permanent
`400`.

It is a post-monomorphization error, so the diagnostic points into the handler body with an
instantiation chain back to the mount site — and, for the same reason, **only a real build
reports it.** `cargo build` and `cargo test` evaluate the constant; `cargo check` and an editor
running it do not, so a transposed pair looks fine in the editor and fails in CI. This is the
same reason the root `CLAUDE.md` says `cargo check` is not a substitute for `cargo test` here. No
test asserts the failure: that would need `trybuild` as a dev-dependency, which the
`[dev-dependencies]` comment convention would have to justify for a single compile-fail case.

**The container `serde` attribute names its path as a string.** `#[serde(default = "OffsetParamsQuery::<DEFAULT_LIMIT, MAX_LIMIT>::default")]`
is not a style choice: a bare `#[serde(default)]` makes `serde_derive` add a `Self: Default`
predicate and panic outright with "Serde does not support const generics yet". That means the
struct name and both const parameter names are load-bearing in the same way crate names and
root re-export positions are — rename either and deserialization breaks with an unresolved-path
error inside generated code. `Self::default` does not work there either; `Self` is serde's
internal `__Visitor`.

Because the default arrives through `Default`, `limit` needs no `Option`: a request naming no
limit deserializes straight to `DEFAULT_LIMIT`, and an explicit `?limit=0` still survives to be
rejected.

## The policy bounds page size, not depth

There is deliberately no ceiling on `offset`. `?offset=4294967295` is a legal request and hands a
very deep `OFFSET` to Postgres, which scans and discards every skipped row. `sql_traits` must not
clamp — a silently reduced page is indistinguishable from a short last page, and the same argument
covers a silently reduced offset — and an implementation's only error channel there is
`sqlx::Error`, which is a `500`. A consumer who needs to bound depth does it in their own query
(keyset pagination, or a rejection before the SQL runs), not here; cursor mode exists precisely
because deep offsets are the problem it solves.

## Why the paginated routes extract a `Result<Query<Q>, QueryRejection>`

A bare `Query<Q>` rejection renders as axum's plain-text default, which would make an
unparseable `?limit=abc` the one error in this crate whose body is not `{"message": "..."}`.
Extracting the `Result` moves that rendering into the handler, once, for every implementor —
the same argument as the empty-update `400`.

Both paginated handlers unwrap it through the private `validated_query`, which widens the
rejection and runs `validate` in one place — the same role `optional_record_response` plays for
the fetch-one handlers. A new paginated handler calls it rather than repeating the two branches.
It hands back an `ApiError` rather than a finished `Response`, so the rendering stays in
`From<ApiError> for ApiErrorResponse` with every other error; see **Errors** below.

## The `*Where` family has no derive

`GetRecordWhereRoute`, `ListRecordsWhereRoute` and `DeleteRecordsWhereRoute` each declare a
`PathParams` associated type bounded `Into<T>`, which has to be chosen by the implementor and
cannot be inferred from the struct. They are therefore invisible to `crates/macros` and to
`tests/derive_macros.rs`; `tests/route_traits.rs` is their only coverage.

## Errors

`ApiError` (via `error_set!`) is the handler-facing error; `ApiErrorResponse` is the
status-plus-JSON-message rendering, and every error body is `{"message": "..."}`. `IOError`
is the server's own I/O failing and both its variants are a `500` — `sqlx` errors and
`serde_json` errors alike.

**A `serde_json` error is a `500`, not a `400`.** It looks like the client's fault and is not:
no request this crate serves can reach that variant. A JSON body is deserialized by axum's
`Json` extractor, which rejects with its own `400` before the handler is called, and a query
string arrives as `ApiError::InvalidQueryParams`. What is left is the server failing to
serialize a value of its own, which the client can do nothing about. This replaced a standing
TODO that assumed the variant had to be split into a deserialization `400` and a serialization
`500`; the deserialization half never arrives, so there is nothing to split. The rule that
keeps it true is `IOError`'s membership: a variant that *can* be the client's fault goes in
`ValidationError` or inline on `ApiError`, not here.

`ValidationError` is the validation subset: `InvalidPaginationLimit { requested, max }` today. It
is a subset rather than inline `ApiError` variants so `PaginationQuery::validate` can return only
what it can actually produce; the handler converts it with `ApiError::from`. Note
`UpdateRoute`'s empty-body `400` is still an inline `ApiErrorResponse` and is the standing
exception to that.

**What may join `ValidationError`, and what may not.** It derives `PartialEq`/`Eq`, which is what
lets `pagination_params.rs` assert a validation result with `assert_eq!` instead of `matches!`,
so a variant holding a source type that is not comparable would cost every one of those
assertions. `ApiError::InvalidQueryParams(QueryRejection)` is the case that came up and the
reason it is inline on `ApiError` despite being the client's fault: `QueryRejection` is neither
`PartialEq` nor `Eq`, and no validation function can produce one — it arrives already formed
from the extractor, so putting it in the subset would widen `validate`'s return type to
something it can never return. Client-caused is the theme of `ValidationError`, not its rule; the
rule is *what validation produces, comparably*.

It needs no arm of its own in `From<ApiError> for ApiErrorResponse`. `error_set` renders an
undecorated source variant as the source's own `Display`, and a `QueryRejection`'s `Display` is
by definition its `body_text()` — so `value.to_string()` yields exactly the string axum would
have sent as plain text, and it lands in this crate's message object instead. If a future
`QueryRejection` variant carries a status other than `400`, that arm becomes
`FlexibleError(rejection.status(), rejection.body_text())`; it is `#[non_exhaustive]`, so this
is worth rechecking on an axum upgrade.

`validated_query` returns `Result<Q, ApiError>` and not a rendered `Response` for that reason —
both failures are already `ApiError` variants, so each arrives with `?` and the rendering stays
in the one `From` impl. It is also what keeps `clippy::result_large_err` quiet without an
`#[allow]`: a `Response` is 128 bytes, the lint's own threshold, and an `ApiError` is 48. If a
future variant pushes it past 128 the lint returns, and the fix is to shrink the variant, not to
silence it.

## Tests

Four crates, deliberately split by what they can prove:

- `derive_macros.rs` — the derives' output compiles and type-checks with only `axum_helpers`
  and `macros` in scope. Derived impls are checked whether or not they are used, so their
  existence forces every generated path to resolve. It has no `GetLatestRecord` impl on
  purpose: if `BasicCrudRoutes` started bundling `GetLatestRoute` again, this file would stop
  compiling.
- `pagination_params.rs` — the query types in isolation, driven directly with
  `Query::try_from_uri` rather than through a mounted handler. Its subject is the type: what a
  URL deserializes into, what `Default` fills in, what `validate` rejects, and what `From`
  hands to the SQL layer.
- `route_traits.rs` — every handler mounted on a real `Router`. Implementing a route trait
  and mounting it are separate checks; `Router::route` is where axum's `Handler` requirements
  are actually enforced, and it is where the pre-v0.7.0 missing-`DeserializeOwned` bug
  surfaced. This is also the only file covering the `*Where` family.
- `route_responses.rs` — what handlers actually answer. Driving a `Router` with a real
  request is the only way to exercise URL-segment binding: calling a handler directly takes a
  `Path` built by hand, which proves nothing about which segment filled which field.
