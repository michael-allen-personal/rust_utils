# sql_traits

Database traits generic over the `sqlx` driver, plus the type-level associations the
`sql_traits_macros` derives and the `axum_helpers` route traits are both built on. Every
trait here is a contract a consumer implements; this crate holds no queries and opens no
connections.

**Keep `axum` out.** The layering is one-way — `axum_helpers` depends on this crate, never
the reverse — so a non-HTTP consumer can use the database traits alone. Anything about
status codes, extractors, or request shape belongs upstream in `axum_helpers`.

`sqlx`, `serde` and `async_trait` are re-exported because their types appear in these
signatures; see the root `CLAUDE.md` for why that matters and why generated code reaches
`serde` as `::sql_traits::serde`. This crate does not pick a `sqlx` runtime feature, and it
enables no driver either — both are the consumer's choice. (`axum_helpers` enables
`runtime-tokio`, `postgres` and `sqlite` in its dev-dependencies only, for the same reason.)
The `postgres`/`sqlite`/`mysql`/`any` features on this crate do nothing but forward to the
matching `sqlx` feature, so a consumer reaches its driver through the re-exported `sqlx`
instead of declaring a second `sqlx` of its own just to flip one on.

## The four derives: `PrimaryKey`, `Record`, `Update`, `Database`

They come from `sql_traits_macros`, a private crate this one depends on and re-exports the
derives from — consumers never declare `sql_traits_macros` themselves. The first three read
`#[sql_traits(...)]` and emit new types plus the association impls below. Reserved names:
`{Name}PrimaryKey`, `{Name}Body`, `{Name}Update`. `Database` reads
`#[sql_traits(database = Db)]` and emits only the `HasDatabase` impl — no new type, so it
reserves no name of its own.

`Record` and `Update` are designed to be derived together. `PrimaryKey` alongside `Record`
is not: both emit `impl HasPrimaryKey`, so it is a pile of duplicate-item errors. `Database`
is designed to be derived alongside any of the other three — it emits a different impl
(`HasDatabase`) and carries its own directive, and `Record`/`Update`/`PrimaryKey` each
tolerate `#[sql_traits(database = Db)]` on the struct without consuming it.

### `#[sql_traits(...)]` errors, never skips

Exactly four directives: `primary_key` on a field, and `body_derive(...)`,
`update_derive(...)` and `database = Db` on the struct — `Db` one of `Postgres`, `Sqlite`,
`MySql`, `Any`. Anything else — a typo, a stray comma, a non-list form, a struct directive
written on a field — is a `compile_error!` naming what was found and what is accepted.
Errors are accumulated, so several bad attributes all report at once.

Silence here was actively dangerous, not merely unhelpful: a malformed
`#[sql_traits(primary_key,)]` used to drop the field out of the key *and* put it into the
generated request body — the exact key leak the body type exists to prevent, and one no
round-trip test can catch, because the isomorphism still holds when the partition is wrong.
`database` has no such silent-fallback failure mode to guard against, but it is still a hard
error when absent rather than a default: a default would hide which database a record
targets, and would fail with a message about Postgres on a consumer that never enabled that
driver.

`Record` and `Update` tolerate *each other's* struct-level list, since a derive macro cannot
see its siblings and so cannot tell a typo from a directive meant for the other one. All
three generators additionally tolerate `database`, which none of them reads themselves —
it is present only for `Database` to read when derived alongside one of them.

### Why derive lists have to be named explicitly

rustc strips sibling `#[derive(...)]` attributes before a derive macro runs, so the derives
for a generated type cannot be copied and must be spelled out in `body_derive` /
`update_derive`. *Other* attributes are forwarded automatically — everything except this
crate's own `#[sql_traits(...)]` — which is what keeps `serde` and `ts-rs` renames aligned
between a record and its body. Note that includes `#[serde(deny_unknown_fields)]`: a record
carrying it yields a body that rejects a payload containing the primary key. That is
deliberate; an explicit opt-in to strictness is honoured rather than quietly overridden.

The one exception is `{Name}PrimaryKey`, which forwards nothing and carries a fixed derive
set (`Clone`, `Debug`, `PartialEq`, `Deserialize`). Its wire format is URL path segments, not
JSON, so a `rename_all` meant for a request body has no business renaming the segments a
route must declare.

### Two decisions to preserve

**A composite key is a named struct, not a tuple.** `axum::extract::Path` fills a tuple from
URI segments left to right with no name matching, so a route declaring its segments in a
different order from the marked fields compiles, mounts, runs — and addresses the wrong row.
A struct binds by name. A single marked field stays its own bare type: one path segment binds
unambiguously, and wrapping it would break every `get_record(&pool, 5)` for nothing. A
composite key on a tuple struct has no field names to bind to, so it is a compile error
rather than a silent fallback.

`PrimaryKeyShape` builds the associated type, the accessor, and the destructuring pattern
together, and is shared by `PrimaryKey` and `Record` — so a key field cannot be rebuilt into
the wrong slot. Keep it that way. It clones marked fields (fully qualified, so clippy's
`clone_on_copy` stays quiet on `i64` keys), which is why they must be `Clone`.

**`is_option` is syntactic and that is fine.** It matches the last path segment, so
`Option<T>`, `std::option::Option<T>` and `::core::option::Option<T>` all count; a type
*alias* for `Option<T>` does not, and no proc macro can resolve one. The blast radius is
contained by design: the generated field still gets the right type, and only loses its
`deserialize_with`, so an explicit `null` reads as "leave alone" rather than "clear". A wrong
answer is never a type error, only a missing capability. Do not try to make it smarter.

## `HasDatabase` is what makes every pool-taking trait generic

`HasDatabase { type Database: sqlx::Database; }` associates a type with the one database its
queries run against. The database travels as an associated type rather than a parameter on
each trait, so a record names its database exactly once and the thirteen pool-taking traits
in this crate (eleven here, two more in `pagination.rs`) all take
`&Pool<<Self as HasDatabase>::Database>` — every trait's parameter list, and every
`axum_helpers` mount site, stays exactly the shape it was before this type existed.

**Deliberately not a supertrait of `HasPrimaryKey`.** The key/request-body/update-fields
machinery — `HasPrimaryKey`, `HasRequestBody`/`RequestBody`, `HasUpdateFields`/
`UpdateFields` — never touches a pool. Tying it to `HasDatabase` would force a consumer of
those traits alone, one with no query to run, to name a database it does not have.

`sql_traits::Database` implements this from `#[sql_traits(database = Postgres)]` (or
`Sqlite`, `MySql`, `Any`) — see "The four derives" above. The directive is required rather
than defaulted, so a consumer always states which driver a record targets rather than
inheriting one silently.

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

`sql_traits::Record` and `sql_traits::Update` emit both sides of their pair together, so
hand-written impls are the only way to get them out of step.

**No side carries the primary key.** It comes from the URL path, so there is never a second
key to reconcile with it. A malformed `#[sql_traits(primary_key)]` that silently demoted a
field would leak the key into the generated body — which is why `sql_traits_macros`
hard-errors on anything it does not recognize rather than skipping it.

## Three states in a partial update

An update field wraps the record's type in one more `Option`, so `String` becomes
`Option<String>` and `Option<i32>` becomes `Option<Option<i32>>`. That outer `Option`
distinguishes absent (leave alone) from present, and for a nullable column the inner one
distinguishes `null` (clear it) from a value.

serde collapses the middle case on its own: a plain derive on `Option<Option<T>>` reads both
a missing key and an explicit `null` as `None`. `double_option` is what keeps them apart, and
it only works paired with `#[serde(default)]` — the `Some` it always returns means "the key
was present", and `default` supplies the `None` that means it was not. `sql_traits::Update`
emits both attributes on every field it can see is nullable. A hand-written update type that omits
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
  it is a contract. `tests/pagination.rs`'s `Entry` fixture exercises it against a real
  in-memory database, and the fixture in `axum_helpers/tests/route_responses.rs` is written
  to comply too — both show the envelope carries the distinction, and say nothing about
  whether every consumer's impl honours it. Nothing anywhere enforces it.
- **No limits are enforced here.** Parameters arrive already validated; `axum_helpers` owns the
  policy and a direct non-HTTP caller is trusted. Do not clamp — a silently reduced page is
  indistinguishable from a short last page.

Neither `OffsetParams` nor `CursorParams<C>` has a `Default`, deliberately. They carry no limit
policy, so any default limit here would be a magic number every consumer inherits — the
crate-wide fallback that `axum_helpers`' const-generic query types exist to avoid. A caller
building parameters by hand states the limit it means.

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

Six crates under `tests/`, each proving something the others cannot:

- `derive_macros.rs` — the consumer-perspective compile test. It names neither `serde` nor
  `sqlx`, reaching them through this crate's re-exports, so building at all is the assertion
  that the derives' generated paths resolve from outside. Its `Update` section adds runtime
  checks, because a `deserialize_with` that resolves but is not actually attached would still
  compile and would still lose an explicit `null`.
- `double_option.rs` — the three states, over real JSON.
- `database_generic.rs` — every pool-taking trait implemented against SQLite rather than
  Postgres, purely to prove the traits and their `Pool<<Self as HasDatabase>::Database>`
  signatures are not secretly tied to one driver. All thirteen appear in this one file — the
  eleven at the crate root plus `ListRecordsPaginated`/`ListRecordsWherePaginated` from
  `pagination.rs`, one at `OffsetParams` and the other at `CursorParams<i64>` so both
  pagination modes are exercised — with a `WidgetFilter` standing in for a `*Where` clause
  and `sql_traits::Update` supplying the `HasUpdateFields`/`UpdateFields` pair `UpdateRecord`
  needs. Nothing here connects: these are compile-time assertions, and `pagination.rs` and
  `axum_helpers/tests/route_responses.rs` are where real queries run.
- `pagination.rs` — the envelope's JSON wire shape through this crate's own re-exported
  `serde`: one outer shape for both modes, the cursor keeping its own JSON type rather than
  being stringified. A `Widget` fixture proves `ListRecordsPaginated` and
  `ListRecordsWherePaginated` are implementable from outside the crate as a compile-time
  assertion — no rows, nothing awaited — and a second fixture, `Entry`, runs real queries
  against a real in-memory SQLite database to exercise the contract a fake impl cannot be
  held to: a cursor's total order, `next` coming from a `limit + 1` fetch, and `total` being
  `Some` if and only if it was asked for.
- `request_body_traits.rs` / `update_fields_traits.rs` — the pair contracts, including the
  `record -> (key, body) -> record` round trip that catches a field reassembled into the
  wrong slot.

See the root `CLAUDE.md` for why these live in `tests/` rather than a `#[cfg(test)]` module.

`sql_traits_macros`' own tests are the opposite shape: unit tests in its `src/lib.rs`, the
exception to that rule. They assert on the token strings the private `expand_*` functions
return, which is only reachable in-crate — it is why `proc_macro::TokenStream` appears
solely in the `#[proc_macro_derive]` signatures there, with every expansion function taking
and returning `proc_macro2::TokenStream`. Token-string assertions cannot tell you whether
the output *resolves*; that half is covered from outside, by `sql_traits/tests/derive_macros.rs`,
`axum_helpers/tests/derive_macros.rs` and `generic_helpers/tests/derive_macros.rs`. A change
to an emitted path needs both.
