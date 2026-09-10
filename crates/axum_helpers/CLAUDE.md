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
