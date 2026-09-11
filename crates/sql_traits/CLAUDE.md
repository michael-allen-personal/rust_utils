# sql_traits

Database traits over `sqlx`/Postgres, plus the type-level associations the `macros` derives
and the `axum_helpers` route traits are both built on. Every trait here is a contract a
consumer implements; this crate holds no queries and opens no connections.

**Keep `axum` out.** The layering is one-way — `axum_helpers` depends on this crate, never
the reverse — so a non-HTTP consumer can use the database traits alone. Anything about
status codes, extractors, or request shape belongs upstream in `axum_helpers`.

`sqlx`, `serde` and `async_trait` are re-exported because their types appear in these
signatures; see the root `CLAUDE.md` for why that matters and why generated code reaches
`serde` as `::sql_traits::serde`. This crate does not pick a `sqlx` runtime feature — that
is the consumer's choice. (`axum_helpers` enables `runtime-tokio` in its dev-dependencies
only, for the same reason.)

## The paired-trait pattern

Three of the traits come in `Has*`/side pairs, and the shape repeats exactly:

| Record side | Other side | Carries |
|---|---|---|
| `HasPrimaryKey` | *(none)* | the key |
| `HasRequestBody` | `RequestBody` | every non-key field |
| `HasUpdateFields` | `UpdateFields` | every non-key field, each optional |

The pairing is deliberately **mutual**: `RequestBody::Record` is bound
`HasRequestBody<RequestBody = Self>`, and `UpdateFields::Record` likewise. A body type can
only name a record that names it back, so the two directions cannot drift onto different
types — and that same bound is what makes `with_key` and `apply` provided methods, leaving
an implementor with an associated type and (for `UpdateFields`) `is_empty`.

`macros::Record` and `macros::Update` emit both sides of their pair together, so hand-written
impls are the only way to get them out of step.

**No side carries the primary key.** It comes from the URL path, so there is never a second
key to reconcile with it. A malformed `#[macros(primary_key)]` that silently demoted a field
would leak the key into the generated body — which is why `macros` hard-errors on anything
it does not recognize rather than skipping it.

## Three states in a partial update

An update field wraps the record's type in one more `Option`, so `String` becomes
`Option<String>` and `Option<i32>` becomes `Option<Option<i32>>`. That outer `Option`
distinguishes absent (leave alone) from present, and for a nullable column the inner one
distinguishes `null` (clear it) from a value.

serde collapses the middle case on its own: a plain derive on `Option<Option<T>>` reads both
a missing key and an explicit `null` as `None`. `double_option` is what keeps them apart, and
it only works paired with `#[serde(default)]` — the `Some` it always returns means "the key
was present", and `default` supplies the `None` that means it was not. `macros::Update` emits
both attributes on every field it can see is nullable. A hand-written update type that omits
them silently turns "clear this column" into "leave it alone", with no type error anywhere.

`double_option` is `#[doc(hidden)]` and public only because generated code has to name it.

## `Ok(None)` is a missing row, not an error

`GetRecord`, `ReplaceRecord` and `UpdateRecord` return `Result<Option<Self>, _>`. A key
matching no row is `Ok(None)`, which is what lets `axum_helpers` answer `404`. Returning
`Result<Self, _>` would force implementations to surface a missing row as
`sqlx::Error::RowNotFound`, and the error mapping turns any `sqlx::Error` into a `500`.

Keep new fetch-one traits on that convention.

## Pagination is two traits, because the mode is a type parameter

`ListRecordsPaginated<P>` and `ListRecordsWherePaginated<T, P>` take the validated parameters
as `P`, so offset and cursor mode are the same trait at `OffsetParams` and `CursorParams<C>`
rather than four traits. `P::Pagination` is what the response carries, which is what lets one
`axum_helpers` handler serve every mode: the metadata comes back from the query, and HTTP code
could not have produced a total or a next cursor anyway.

`CursorPagination<C>` is generic over the cursor, so `next` goes back out as the type that came
in. A consumer whose cursor is a row id pays nothing; `String` is for an impl that genuinely
needs an opaque composite cursor. Opacity is the implementor's decision, not this crate's.

**A malformed cursor is decoded in `C`, not in the impl.** `list_records_paginated` returns
`Result<_, sqlx::Error>` and `axum_helpers` maps every `sqlx::Error` to a `500`, so an impl handed
client-supplied garbage in an opaque cursor has no way to answer `400` — there is no error variant
for it and adding one would put request validation in this crate. Put the decoding in the cursor
type's own `Deserialize` instead: make the opaque cursor a type whose `Deserialize` base64-decodes
and parses, rather than a bare `String` the impl unpacks later. A cursor that does not decode is
then a query-string rejection, which `axum_helpers` renders as its `400` before the handler runs,
and by the time `CursorParams<C>` reaches an impl the cursor is as validated as the limit is.

Four things the doc comments carry, each a silent wrong answer rather than a compile error:

- **A cursor needs a total order.** `ORDER BY created_at` with ties skips and duplicates rows
  across pages. End the sort with a unique tiebreaker and encode the whole sort key.
- **`next` comes from fetching `limit + 1` and dropping the extra.** Anything else guesses or
  pays for a second count.
- **`total` is `Some` if and only if `include_total`.** The compiler cannot enforce an iff, so
  it is a contract. It is exercised by the test fixture in
  `axum_helpers/tests/route_responses.rs`, which is written to comply — that shows the envelope
  carries the distinction, and says nothing about whether a consumer's impl honours it. Nothing
  anywhere enforces it.
- **No limits are enforced here.** Parameters arrive already validated; `axum_helpers` owns the
  policy and a direct non-HTTP caller is trusted. Do not clamp — a silently reduced page is
  indistinguishable from a short last page.

`total: u32` means an implementation casts `count(*) OVER ()`'s `i64`. That is the
implementation's business, not the trait's.

## Writing an `UpdateRecord` impl

An update whose `SET` list depends on which fields are present cannot be a single
`query_as!`. Two shapes work: `sqlx::QueryBuilder`, pushing a binding per `Some` field; or
fetch-apply-replace via `UpdateFields::apply`, trading a round trip for the compile-time
checked macros. Neither needs to handle the empty case — `axum_helpers::UpdateRoute` rejects
an empty body with `400` before calling, since an empty `SET` list is a syntax error rather
than a no-op.

## Tests

Five crates under `tests/`, each proving something the others cannot:

- `derive_macros.rs` — the consumer-perspective compile test. It names neither `serde` nor
  `sqlx`, reaching them through this crate's re-exports, so building at all is the assertion
  that the derives' generated paths resolve from outside. Its `Update` section adds runtime
  checks, because a `deserialize_with` that resolves but is not actually attached would still
  compile and would still lose an explicit `null`.
- `double_option.rs` — the three states, over real JSON.
- `pagination.rs` — the envelope's JSON wire shape through this crate's own re-exported
  `serde`: one outer shape for both modes, the cursor keeping its own JSON type rather than
  being stringified. Also proves `ListRecordsPaginated` and `ListRecordsWherePaginated` are
  implementable from outside the crate — a compile-time assertion, since this crate has no
  async runtime in its dev-dependencies and the impls are never awaited.
- `request_body_traits.rs` / `update_fields_traits.rs` — the pair contracts, including the
  `record -> (key, body) -> record` round trip that catches a field reassembled into the
  wrong slot.

See the root `CLAUDE.md` for why these live in `tests/` rather than a `#[cfg(test)]` module.
