# Database-generic SQL and route traits

**Date:** 2026-09-11
**Status:** Approved, ready for implementation planning

## Problem

Every pool-taking trait in `sql_traits` hard-codes `&PgPool`, and every route trait in
`axum_helpers` hard-codes `State<PgPool>`. A consumer that wants SQLite or MySQL cannot use
this workspace at all, and the immediate motivation is a sample API that would be far
cheaper to build against an in-memory SQLite database than against Postgres.

The constraint is that the ergonomics must not regress. These traits are implemented by hand
in consumer repos, roughly eight impls per record type, and mounted as axum handlers. A
design that forces a database to be named in every impl, or a turbofish at every mount site,
costs more than it buys.

## Goals

- Any `sqlx`-supported database works behind the SQL traits and the route traits.
- Trait parameter lists do not grow; `ListRecordsWhereRoute<T>` stays `ListRecordsWhereRoute<T>`.
- Mount sites stay turbofish-free.
- A record names its database exactly once.
- A SQLite-only consumer never compiles `sqlx-postgres`.

## Non-goals

- **Generalizing over `Executor` rather than `&Pool`.** Accepting a transaction or a
  `&mut Connection` is a separate, orthogonal change whose bounds
  (`for<'c> &'c mut DB::Connection: Executor<'c, Database = DB>`) would spread across every
  signature. Deliberately deferred; if it is ever wanted, it is its own breaking change.
- **One record type serving two databases at once.** Explicitly rejected during design: the
  associated-type approach forecloses it, and a record needing a second backend uses a
  newtype.
- **`sqlx::Any` as the mechanism.** Runtime driver dispatch costs lowest-common-denominator
  type mapping (no `Uuid`, no `Json`), requires `install_default_drivers()`, and discards
  per-driver compile-time query checking. `Any` remains *nameable* as a database choice, but
  it is not how genericity is achieved.

## Design

### 1. `HasDatabase`

```rust
// sql_traits
/// Associates a type with the one database its queries run against.
pub trait HasDatabase {
    type Database: ::sqlx::Database;
}
```

The database travels as an associated type rather than a trait parameter. That is the whole
mechanism, and it is what keeps parameter lists and mount sites unchanged.

`HasDatabase` is deliberately **not** a supertrait of `HasPrimaryKey`. The key/body/update
machinery (`HasPrimaryKey`, `HasRequestBody`, `RequestBody`, `HasUpdateFields`,
`UpdateFields`, `double_option`) never touches a pool and stays database-agnostic, so a
non-SQL consumer of those traits is unaffected.

Write the bound as `::sqlx::Database` fully qualified. `type Database: Database` inside
`sql_traits` reads recursively and invites confusion with the associated type.

### 2. The thirteen SQL traits

Each gains `HasDatabase` as a supertrait, and `pool: &PgPool` becomes
`pool: &Pool<<Self as HasDatabase>::Database>`. Nothing else changes — in particular
**every return type is untouched**, because `sqlx::Error` is one type for all drivers rather
than being generic over them.

| Trait | New supertrait? |
|---|---|
| `GetLatestRecord` | yes |
| `GetRecord` | yes (alongside `HasPrimaryKey`) |
| `GetRecordWhere<T>` | yes |
| `ListRecords` | yes |
| `ListRecordsWhere<T>` | yes |
| `ListRecordsPaginated<P>` | yes |
| `ListRecordsWherePaginated<T, P>` | yes |
| `InsertRecord` | yes |
| `BulkInsertRecords` | yes |
| `ReplaceRecord` | yes (alongside `HasPrimaryKey`) |
| `UpdateRecord` | yes (alongside `HasUpdateFields`) |
| `DeleteRecord` | yes (alongside `HasPrimaryKey`) |
| `DeleteRecordsWhere<T>` | yes |

Example:

```rust
#[async_trait]
pub trait GetRecord: Sized + HasPrimaryKey + HasDatabase {
    async fn get_record(
        pool: &Pool<<Self as HasDatabase>::Database>,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error>;
}
```

Use the fully qualified `<Self as HasDatabase>::Database` form, matching the existing
`<Self as HasPrimaryKey>::PrimaryKey` style.

### 3. The thirteen route traits

`State<PgPool>` becomes `State<Pool<<Self as HasDatabase>::Database>>`. Handler bodies,
supertrait lists, `type PathParams`, and every `where` clause are otherwise unchanged.
`HasDatabase` is reached through each route trait's existing SQL supertrait, so no route
trait needs to name it as a supertrait itself.

Affected: `GetLatestRoute`, `GetRecordRoute`, `GetRecordWhereRoute<T>`, `ListRecordsRoute`,
`ListRecordsPaginatedRoute<Q>`, `ListRecordsWhereRoute<T>`,
`ListRecordsWherePaginatedRoute<T, Q>`, `CreateRoute<'de>`, `BulkCreateRoute<'de>`,
`DeleteRoute`, `DeleteRecordsWhereRoute<T>`, `ReplaceRoute`, `UpdateRoute`.

```rust
#[async_trait]
pub trait GetRecordRoute: GetRecord + serde::Serialize
where
    <Self as HasPrimaryKey>::PrimaryKey: DeserializeOwned,
{
    async fn get_record_route(
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
        Path(primary_key): Path<<Self as HasPrimaryKey>::PrimaryKey>,
    ) -> Response { /* body unchanged */ }
}
```

Mount sites are unchanged and need no turbofish:

```rust
Router::new()
    .route("/widgets/{id}", get(Widget::get_record_route))
    .with_state(sqlite_pool)
```

`ApiError`, `ApiErrorResponse`, `IOError`, `ValidationError` and the whole of
`axum_helpers::pagination` are untouched.

### 4. `macros::Database`

A new standalone derive, independent of `Record` and `PrimaryKey`, so that hand-written
records and separate insert-side types can use it too.

```rust
#[derive(macros::Database, macros::Record, macros::BasicCrudRoutes)]
#[macros(database = Sqlite, body_derive(Deserialize))]
struct Widget {
    #[macros(primary_key)]
    id: i64,
    name: String,
}
```

emits, per the absolute-path rule:

```rust
impl ::sql_traits::HasDatabase for Widget {
    type Database = ::sql_traits::sqlx::Sqlite;
}
```

Rules:

- `database = <Ident>` joins the `#[macros(...)]` directive enum and is added to the
  directive list named in error messages.
- Accepted idents: `Postgres`, `Sqlite`, `MySql`, `Any`. Any other ident is a hard error
  naming the accepted set, consistent with how `parse_macros_attr` already rejects unknown
  directives rather than skipping them.
- The directive is **required** by this derive. Its absence is a compile error, never a
  silent default to Postgres.
- `Record` and `Update` **tolerate** the `database` directive without consuming it, exactly
  as they already tolerate each other's `body_derive`/`update_derive` lists. `Database`
  likewise tolerates theirs.
- `Database` emits nothing for the generated `{Name}Body`. In this workspace `InsertRecord`
  is implemented on the **record** — that is what `expand_insert_route` emits `CreateRoute`
  for — so the body never needs `HasDatabase`. A consumer with a separate `NewWidget` insert
  type derives `Database` on that type directly.

A consumer with a custom `sqlx::Database` implementation writes the three-line impl by hand;
the derive covers the drivers `sqlx` ships.

### 5. Cargo features

`sql_traits` and `axum_helpers` name no concrete driver after this change, so they compile
with **no driver feature at all**. The features below exist purely so consumers can enable a
driver while still depending on the re-exported `sqlx` rather than declaring it themselves,
which is what the re-export convention asks of them.

```toml
# sql_traits
[features]
postgres = ["sqlx/postgres"]
sqlite   = ["sqlx/sqlite"]
mysql    = ["sqlx/mysql"]
any      = ["sqlx/any"]

# axum_helpers
[features]
postgres = ["sql_traits/postgres", "sqlx/postgres"]
sqlite   = ["sql_traits/sqlite",   "sqlx/sqlite"]
mysql    = ["sql_traits/mysql",    "sqlx/mysql"]
any      = ["sql_traits/any",      "sqlx/any"]
```

No default feature: a SQLite-only consumer then never compiles `sqlx-postgres`.

The workspace `sqlx` dependency drops its `features = ["postgres"]`:

```toml
sqlx = { version = "0.9" }
```

The existing stance that neither crate picks a `sqlx` *runtime* feature is unchanged — that
remains the consumer's choice.

### 6. Tests

Test crates activate drivers through their own `sqlx` dev-dependency features, which
unify across the build. Test files keep reaching every type through the crate-under-test's
re-exports (`axum_helpers::sqlx::Sqlite`, not a bare `sqlx::Sqlite`), so the external-contract
assertion is preserved.

**Converted to real in-memory SQLite:**

- `axum_helpers/tests/route_responses.rs` — its fixtures' SQL impls become real queries
  against a real table. Today every SQL impl in this file is a fake that ignores the pool;
  the file's subject is what handlers actually answer, so real tables and real queries
  strengthen it directly. All 25 tests keep their existing assertions and simply run against
  real rows; the few that never reach the database (the empty-update `400`, for instance,
  which is rejected before `update_record` is called) are unaffected beyond the fixture
  change. A helper builds a `SqlitePool` and
  creates the schema (`widget(id INTEGER PRIMARY KEY, name TEXT NOT NULL)`); the existing
  `MISSING_ID` convention becomes a key that genuinely matches no row. Assertions stay as
  they are — status codes and bodies.
- `sql_traits/tests/pagination.rs` — the existing wire-format tests stay as they are, and
  real SQLite-backed `ListRecordsPaginated` tests are added for the parts of the contract
  only a database can prove: offset paging, `include_total` filled if and only if requested,
  cursor paging fetching `limit + 1` with `next: None` on the last page, and a cursor with a
  unique tiebreaker neither skipping nor duplicating a row across page boundaries.

**Stay compile-only:**

- `axum_helpers/tests/derive_macros.rs`, `axum_helpers/tests/route_traits.rs`,
  `sql_traits/tests/derive_macros.rs`, `sql_traits/tests/request_body_traits.rs`,
  `sql_traits/tests/update_fields_traits.rs`. Their purpose is proving generated paths
  resolve and handlers mount from outside the crate; a database would dilute that rather than
  strengthen it. Their stub impl signatures update to the new pool type, and each gains its
  `HasDatabase` impl. The `sql_traits` fixtures use `Sqlite`, that being the only driver its
  dev-dependencies enable; the two-database assertion belongs to `axum_helpers` (below), so
  `sql_traits` does not need a second driver in its dev-dependencies.
- `sql_traits/tests/double_option.rs` and `axum_helpers/tests/pagination_params.rs` never
  touch a pool and do not change.

**Genericity is asserted at the consumer boundary:** `axum_helpers/tests/route_traits.rs`
carries fixtures on **two different databases** — one Postgres, one SQLite — so that "these
traits are generic over the database" is a compile-time assertion from a consumer's vantage
point rather than a claim in a doc comment.

Dev-dependency changes:

- `sql_traits`: add `tokio = { workspace = true }` (it has none today) and
  `sqlx = { workspace = true, features = ["sqlite", "runtime-tokio"] }`.
- `axum_helpers`: extend the existing `sqlx` dev-dependency to
  `features = ["sqlite", "postgres", "runtime-tokio"]`.

Declaring `sqlx` in dev-dependencies purely to enable a feature follows the precedent already
set in `axum_helpers`, where the `[dev-dependencies]` comment explains exactly that.

### 7. Migration for consumers

Three steps, and impl bodies are not among them:

1. **`Cargo.toml`** — add a driver feature:
   `sql_traits = { ..., features = ["postgres"] }` and likewise for `axum_helpers`.
2. **Each record type** — add `#[derive(macros::Database)]` with
   `#[macros(database = Postgres)]`, or the three-line impl by hand.
3. **Nothing else.** Existing SQL trait impls compile verbatim: `Pool<Postgres>` *is*
   `PgPool`, so a method written `async fn get_record(pool: &PgPool, ...)` still satisfies
   `&Pool<<Self as HasDatabase>::Database>` once `Database = Postgres`.

### 8. Documentation

- `CHANGELOG.md`, under `## Unreleased`, long-form per this repo's convention:
  - `### Added` — `HasDatabase`; the `macros::Database` derive and its `database` directive;
    the driver features on both crates.
  - `### Changed` — **Breaking.** all thirteen SQL traits and thirteen route traits now take
    `Pool<Self::Database>`; **Breaking.** consumers must enable a driver feature, since
    neither crate enables one any more; **Breaking.** every type implementing a pool-taking
    trait must now implement `HasDatabase`.
- Root `CLAUDE.md` — the crates map and conventions: the traits are database-generic, and
  `sql_traits` picks neither a driver nor a runtime feature.
- `sql_traits/CLAUDE.md` — `HasDatabase` and why it is separate from `HasPrimaryKey`.
- `axum_helpers/CLAUDE.md` — the state type follows the record's database.
- `macros/CLAUDE.md` — the new derive, the `database` directive, the accepted ident set, and
  the tolerate-don't-consume rule shared with `Record`/`Update`.

## Verified during design

Checked in a throwaway probe crate against `sqlx` 0.9 and `axum` 0.8, not assumed:

- `Pool<DB>` is `Clone + Send + Sync + 'static` given only `DB: Database`, so `State<Pool<DB>>`
  needs no extra bounds anywhere.
- A route handler taking `State<Pool<Self::Database>>` satisfies axum's `Handler` and mounts
  with `.with_state(pool)` with no turbofish and no type annotation.
- `sqlx::Error` is not generic over the driver, so no return type in either crate changes.
- `sqlx::Postgres` is behind `feature = "postgres"`, which is why no default type parameter
  is used and why the driver features are opt-in.
- An impl written `async fn list_records(_pool: &PgPool)` satisfies a `&Pool<DB>` signature
  when `DB = Postgres`, which is what makes step 3 of the migration a no-op.
- `sqlx` 0.9 shares one in-memory SQLite database across a pool's connections — a table
  created on one connection is visible from another — so the SQLite test fixtures need no
  `max_connections(1)` or shared-cache URI workaround.

## Risks

- **Feature unification can hide a missing driver feature.** If any crate in a consumer's
  graph enables `sqlx/postgres`, everything sees it, so a consumer who forgets step 1 of the
  migration may build anyway and only discover it when that other dependency changes. The
  two-database fixture in `route_traits.rs` does not catch this; it is called out in the
  CHANGELOG instead.
- **`route_responses.rs` is 709 lines and is being rewritten against a real database.** It is
  the largest single piece of this change and should be its own stage in the implementation
  plan, landing after the trait changes compile.
