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

## Writing an `UpdateRecord` impl

An update whose `SET` list depends on which fields are present cannot be a single
`query_as!`. Two shapes work: `sqlx::QueryBuilder`, pushing a binding per `Some` field; or
fetch-apply-replace via `UpdateFields::apply`, trading a round trip for the compile-time
checked macros. Neither needs to handle the empty case — `axum_helpers::UpdateRoute` rejects
an empty body with `400` before calling, since an empty `SET` list is a syntax error rather
than a no-op.

## Tests

Four crates under `tests/`, each proving something the others cannot:

- `derive_macros.rs` — the consumer-perspective compile test. It names neither `serde` nor
  `sqlx`, reaching them through this crate's re-exports, so building at all is the assertion
  that the derives' generated paths resolve from outside. Its `Update` section adds runtime
  checks, because a `deserialize_with` that resolves but is not actually attached would still
  compile and would still lose an explicit `null`.
- `double_option.rs` — the three states, over real JSON.
- `request_body_traits.rs` / `update_fields_traits.rs` — the pair contracts, including the
  `record -> (key, body) -> record` round trip that catches a field reassembled into the
  wrong slot.

See the root `CLAUDE.md` for why these live in `tests/` rather than a `#[cfg(test)]` module.
