# axum_helpers

One axum route handler per `sql_traits` database trait, as a trait with a provided method,
plus the error type that maps failures onto status codes. A consumer implements the SQL
trait, writes `impl GetRecordRoute for Widget {}` (or derives it), and mounts
`Widget::get_record_route`.

Handlers are provided methods rather than free functions so a type opts in by naming the
trait, and so the supertrait list states the requirements — a missing `GetRecord` impl is an
error on the route trait, not a mystery at the mount site.

Every dependency whose types leak into these signatures is re-exported, `sql_traits`
included; see the root `CLAUDE.md`. All three test crates reach `axum`, `serde`, `sqlx` and
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

## The limit policy lives in the query type

`OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>` carries its policy as const generic parameters, so
a mount site states it where a reader will see it (`impl ListRecordsPaginatedRoute<OffsetParamsQuery<20, 100>> for Invoice {}`)
and the conversion into `sql_traits`' validated types is an infallible, total `From`. The default
is stated once, in `Default`; the maximum is checked by `validate`, which needs no arguments
because it reads the type. Nothing restates either.

The parameters are positional, so a transposed pair is the failure worth catching, and
`POLICY_IS_COHERENT` makes it a compile error. Every genuine transposition trips it, since it
makes the default exceed the maximum — the sole exception is `DEFAULT == MAX`, where transposing
changes nothing. It is a post-monomorphization error, so the diagnostic points into the handler
body with an instantiation chain back to the mount site.

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

## Why the paginated routes extract a `Result<Query<Q>, QueryRejection>`

A bare `Query<Q>` rejection renders as axum's plain-text default, which would make an
unparseable `?limit=abc` the one error in this crate whose body is not `{"message": "..."}`.
Extracting the `Result` moves that rendering into the handler, once, for every implementor —
the same argument as the empty-update `400`.

## The `*Where` family has no derive

`GetRecordWhereRoute`, `ListRecordsWhereRoute` and `DeleteRecordsWhereRoute` each declare a
`PathParams` associated type bounded `Into<T>`, which has to be chosen by the implementor and
cannot be inferred from the struct. They are therefore invisible to `crates/macros` and to
`tests/derive_macros.rs`; `tests/route_traits.rs` is their only coverage.

## Errors

`ApiError` (via `error_set!`) is the handler-facing error; `ApiErrorResponse` is the
status-plus-JSON-message rendering, and every error body is `{"message": "..."}`. `sqlx`
errors become `500`, serde errors `400`. There is a standing TODO on that last one: serde
cannot currently distinguish deserialization (a genuine `400`) from serialization (a `500`).

`RequestError` is the client-input subset: `InvalidPaginationLimit { requested, max }` today. It
is a subset rather than inline `ApiError` variants so `PaginationQuery::validate` can return only
what it can actually produce and widen with `?`. Note `UpdateRoute`'s empty-body `400` is still an
inline `ApiErrorResponse` and is the standing exception to that.

## Tests

Three crates, deliberately split by what they can prove:

- `derive_macros.rs` — the derives' output compiles and type-checks with only `axum_helpers`
  and `macros` in scope. Derived impls are checked whether or not they are used, so their
  existence forces every generated path to resolve. It has no `GetLatestRecord` impl on
  purpose: if `BasicCrudRoutes` started bundling `GetLatestRoute` again, this file would stop
  compiling.
- `route_traits.rs` — every handler mounted on a real `Router`. Implementing a route trait
  and mounting it are separate checks; `Router::route` is where axum's `Handler` requirements
  are actually enforced, and it is where the pre-v0.7.0 missing-`DeserializeOwned` bug
  surfaced. This is also the only file covering the `*Where` family.
- `route_responses.rs` — what handlers actually answer. Driving a `Router` with a real
  request is the only way to exercise URL-segment binding: calling a handler directly takes a
  `Path` built by hand, which proves nothing about which segment filled which field.
