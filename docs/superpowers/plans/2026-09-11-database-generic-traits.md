# Database-Generic SQL and Route Traits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every pool-taking trait in `sql_traits` and every route trait in `axum_helpers` work against any `sqlx`-supported database, without growing any trait's parameter list or requiring a turbofish at a mount site.

**Architecture:** The database travels as an associated type on a new `HasDatabase` trait, which becomes a supertrait of all thirteen pool-taking SQL traits. `&PgPool` becomes `&Pool<<Self as HasDatabase>::Database>`; route handlers take `State<Pool<<Self as HasDatabase>::Database>>`. A record names its database exactly once, via a new `macros::Database` derive. Driver selection moves from a hard-coded workspace feature to opt-in forwarding features on both crates.

**Tech Stack:** Rust edition 2024, `sqlx` 0.9, `axum` 0.8, `async-trait` 0.1, `syn`/`quote` proc macros, `tokio` (tests only).

**Spec:** `docs/superpowers/specs/2026-09-11-database-generic-traits-design.md`

## Global Constraints

- **Macro output names everything by absolute path.** `macros` depends on none of the crates it generates for; it must emit `::sql_traits::HasDatabase`, `::sql_traits::sqlx::Sqlite`, never a bare `sql_traits::`.
- **Leaked-type dependencies are re-exported.** Consumers reach `sqlx`/`serde` through `sql_traits::sqlx` / `axum_helpers::sqlx`. Test files must use those re-export paths, never a bare `sqlx::`.
- **Tests are separate crates that consume the library.** Files under `tests/` compile as their own crates; that external vantage point is the assertion. Do not move assertions into `#[cfg(test)] mod tests`. The single exception is `macros`, whose tests are in-crate and assert on token strings.
- **`cargo check` is not sufficient.** Most assertions live in `tests/` crates that a bare `cargo check` never builds. Run `cargo test` before believing a change works.
- **Neither crate enables a driver feature.** After Task 4, `sql_traits` and `axum_helpers` compile with no `sqlx` driver feature; drivers are opt-in via forwarding features. Neither crate picks a `sqlx` *runtime* feature — that stays the consumer's choice.
- **Accepted database idents:** `Postgres`, `Sqlite`, `MySql`, `Any`. Anything else is a hard compile error naming the set.
- **`sqlx::Error` is not generic over the driver.** No return type in either crate changes.
- **CHANGELOG entries are long-form** under `## Unreleased`, with Keep-a-Changelog subsections and a **Breaking.** prefix on entries requiring downstream edits. They say what changed *and* why, and what a consumer must do.
- **Per-crate `version` fields stay at `0.1.0`.** Releases are tagged `vX.Y.Z` across the workspace.

---

### Task 1: The `macros::Database` derive

`macros` depends on none of the other crates and its tests assert on token strings, so this lands first and in isolation — the emitted `::sql_traits::HasDatabase` path does not need to exist yet.

**Files:**
- Modify: `crates/macros/src/lib.rs`
- Test: `crates/macros/src/lib.rs` (in-crate `mod tests`, per this crate's documented exception)

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `#[derive(macros::Database)]` reading `#[macros(database = <Ident>)]`, emitting `impl ::sql_traits::HasDatabase for #name { type Database = ::sql_traits::sqlx::#ident; }`. Tasks 2, 3, 5 and 6 use this derive in their fixtures.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `crates/macros/src/lib.rs`:

```rust
#[test]
fn database_directive_names_the_sqlx_type_by_absolute_path() {
    let out = expand_db("#[macros(database = Sqlite)] struct Widget { id: i64 }");
    assert!(out.contains(":: sql_traits :: HasDatabase for Widget"), "{out}");
    assert!(
        out.contains("type Database = :: sql_traits :: sqlx :: Sqlite"),
        "{out}"
    );
}

#[test]
fn each_supported_database_is_accepted() {
    for db in ["Postgres", "Sqlite", "MySql", "Any"] {
        let out = expand_db(&format!("#[macros(database = {db})] struct Widget {{ id: i64 }}"));
        assert!(
            out.contains(&format!("type Database = :: sql_traits :: sqlx :: {db}")),
            "{db}: {out}"
        );
    }
}

// A typo must say what to write instead, never silently pick a database.
#[test]
fn an_unknown_database_is_a_hard_error_naming_the_set() {
    let out = expand_db("#[macros(database = Sqlite3)] struct Widget { id: i64 }");
    assert!(out.contains("compile_error"), "{out}");
    assert!(out.contains("Postgres") && out.contains("MySql"), "{out}");
}

// Absence is an error rather than a default, so the database a record targets is
// always visible at the record.
#[test]
fn a_missing_database_directive_is_a_hard_error() {
    let out = expand_db("struct Widget { id: i64 }");
    assert!(out.contains("compile_error"), "{out}");
    assert!(out.contains("database"), "{out}");
}

// `Record` and `Update` must tolerate this directive, and this derive must tolerate
// theirs, because a derive macro cannot see its siblings.
#[test]
fn database_is_tolerated_alongside_the_other_derive_lists() {
    let out = expand_db(
        "#[macros(database = Postgres)] #[macros(body_derive(Deserialize))] \
         #[macros(update_derive(Deserialize))] struct Widget { id: i64 }",
    );
    assert!(!out.contains("compile_error"), "{out}");
    assert!(out.contains("type Database = :: sql_traits :: sqlx :: Postgres"), "{out}");
}

#[test]
fn record_tolerates_the_database_directive() {
    let out = expand_rec(
        "#[macros(database = Sqlite)] #[macros(body_derive(Deserialize))] \
         struct Widget { #[macros(primary_key)] id: i64, name: String }",
    );
    assert!(!out.contains("compile_error"), "{out}");
}

#[test]
fn database_on_a_field_is_reported_as_misplaced() {
    let out = expand_rec(
        "struct Widget { #[macros(primary_key)] id: i64, #[macros(database = Sqlite)] name: String }",
    );
    assert!(out.contains("compile_error"), "{out}");
}
```

`expand_rec` and `expand_upd` already exist in `mod tests` alongside `expand`; only `expand_db`
is new, and it follows their naming:

```rust
fn expand_db(src: &str) -> String {
    expand_database(src.parse().unwrap()).to_string()
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p macros`
Expected: FAIL — `cannot find function 'expand_database' in this scope`.

- [ ] **Step 3: Add the directive to the parser**

In `crates/macros/src/lib.rs`, extend the help string, the directive enum, and the accepted-database list:

```rust
const MACROS_ATTR_HELP: &str = "`#[macros(...)]` accepts exactly `primary_key` on a field \
     and `body_derive(Trait, ...)`, `update_derive(Trait, ...)` or `database = Db` on the \
     struct";

/// The `sqlx::Database` implementations `#[macros(database = ...)]` accepts, named in the
/// error so a typo says what to write instead. A consumer with a database outside this set
/// writes the three-line `HasDatabase` impl by hand.
const DATABASES: [&str; 4] = ["Postgres", "Sqlite", "MySql", "Any"];
```

Add a variant to `MacrosDirective`:

```rust
    /// `#[macros(database = Sqlite)]` — the database this type's queries run against.
    Database(Ident),
```

Add a branch to `parse_macros_attr`, before its `_ => Err(unrecognized())` arm:

```rust
        Ok(Meta::NameValue(name_value)) if name_value.path.is_ident("database") => {
            let syn::Expr::Path(expr) = &name_value.value else {
                return Err(unrecognized());
            };
            let Some(ident) = expr.path.get_ident() else {
                return Err(unrecognized());
            };
            if !DATABASES.contains(&ident.to_string().as_str()) {
                return Err(syn::Error::new_spanned(
                    ident,
                    format!(
                        "unknown database `{ident}`: `database` accepts {}",
                        DATABASES.join(", ")
                    ),
                ));
            }
            Ok(MacrosDirective::Database(ident.clone()))
        }
```

- [ ] **Step 4: Teach the existing readers to tolerate it**

In `is_pk_attr`, add an arm so a field-level `database` is reported as misplaced rather than falling through:

```rust
            MacrosDirective::Database(_) => return Err(misplaced_on_field(attr, "database")),
```

In `container_derives`, add an arm so `Record`, `Update` and `PrimaryKey` all skip it — it is present for the sibling derive that does read it:

```rust
            // Present for the `Database` derive, which is the only reader.
            (MacrosDirective::Database(_), _) => {}
```

- [ ] **Step 5: Add the emitter and the expansion**

```rust
/// Emits the `HasDatabase` impl associating a record with the one database its queries run
/// against. The path is absolute and aimed at `sql_traits`' root plus its `sqlx` re-export,
/// so the generated code resolves at the use site rather than in the caller's module.
fn database_impl(name: &Ident, database: &Ident) -> TokenStream2 {
    quote! {
        impl ::sql_traits::HasDatabase for #name {
            type Database = ::sql_traits::sqlx::#database;
        }
    }
}

/// Reads the struct-level `database` directive. Absence is a hard error rather than a
/// default: a silent fallback would hide which database a record targets, and would fail
/// with a message about Postgres on a consumer that never enabled that driver.
fn container_database(attrs: &[Attribute], name: &Ident) -> syn::Result<Ident> {
    let mut found: Option<Ident> = None;
    for (attr, directive) in macros_directives(attrs)? {
        match directive {
            MacrosDirective::Database(ident) => {
                if found.is_some() {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "`database` is given more than once",
                    ));
                }
                found = Some(ident);
            }
            // Present for a sibling derive that does read it.
            MacrosDirective::BodyDerive(_) | MacrosDirective::UpdateDerive(_) => {}
            MacrosDirective::PrimaryKey => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`primary_key` marks a field, not the struct",
                ));
            }
        }
    }
    found.ok_or_else(|| {
        syn::Error::new_spanned(
            name,
            format!(
                "`#[derive(macros::Database)]` requires `#[macros(database = Db)]`, one of {}",
                DATABASES.join(", ")
            ),
        )
    })
}

/// Body of the `Database` derive.
fn expand_database(input: TokenStream2) -> TokenStream2 {
    let expanded = syn::parse2::<DeriveInput>(input).and_then(|input| {
        let database = container_database(&input.attrs, &input.ident)?;
        Ok(database_impl(&input.ident, &database))
    });
    match expanded {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}
```

Add the derive entry point next to the others:

```rust
/// Derive `Database` — associates a type with the one database its queries run against,
/// named by `#[macros(database = Sqlite)]`.
///
/// Standalone rather than folded into `Record`, so that a hand-written record and a
/// separate insert-side type (a `NewWidget` implementing `InsertRecord`) can both use it
/// without pulling in the body-type generation.
///
/// The directive is required; its absence is a compile error naming the accepted set,
/// never a silent default. `Record` and `Update` tolerate the directive without consuming
/// it, exactly as they already tolerate each other's derive lists.
#[proc_macro_derive(Database, attributes(macros))]
pub fn derive_database(input: TokenStream) -> TokenStream {
    expand_database(input.into()).into()
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p macros`
Expected: PASS, including the seven new tests.

- [ ] **Step 7: Lint and commit**

```bash
cargo clippy -p macros --all-targets && cargo fmt --check
git add crates/macros/src/lib.rs
git commit -m "Add macros::Database derive and the database directive"
```

---

### Task 2: `HasDatabase` and the thirteen SQL traits

**Files:**
- Modify: `crates/sql_traits/src/lib.rs`
- Modify: `crates/sql_traits/src/pagination.rs`
- Modify: `crates/sql_traits/Cargo.toml`
- Modify: `crates/sql_traits/tests/derive_macros.rs`, `crates/sql_traits/tests/request_body_traits.rs`, `crates/sql_traits/tests/update_fields_traits.rs`, `crates/sql_traits/tests/pagination.rs` (fixture signatures only)
- Create: `crates/sql_traits/tests/database_generic.rs`

**Interfaces:**
- Consumes: `#[derive(macros::Database)]` + `#[macros(database = ...)]` from Task 1.
- Produces: `sql_traits::HasDatabase` with `type Database: ::sqlx::Database`. All thirteen SQL traits take `&Pool<<Self as HasDatabase>::Database>`. Task 3 builds route traits on these.

- [ ] **Step 1: Write the failing test**

Create `crates/sql_traits/tests/database_generic.rs` — a consumer-perspective compile test that the traits work for a database other than Postgres:

```rust
//! The SQL traits are generic over the database, asserted from outside the crate.
//!
//! This crate reaches `sqlx` and `serde` only through `sql_traits`' re-exports, so it also
//! proves the re-export surface is enough to implement the traits against a non-Postgres
//! driver. Nothing here connects: the fixtures are compile assertions, and
//! `tests/pagination.rs` is where real queries run.

#![allow(dead_code)]

use sql_traits::async_trait::async_trait;
use sql_traits::sqlx::{self, Pool, Sqlite};
use sql_traits::{
    DeleteRecord, GetLatestRecord, GetRecord, HasDatabase, HasPrimaryKey, InsertRecord,
    ListRecords,
};

#[derive(macros::Database)]
#[macros(database = Sqlite)]
struct Widget {
    id: i64,
    name: String,
}

impl HasPrimaryKey for Widget {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl GetLatestRecord for Widget {
    async fn get_latest_record(_pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl GetRecord for Widget {
    async fn get_record(
        _pool: &Pool<Sqlite>,
        _primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl ListRecords for Widget {
    async fn list_records(_pool: &Pool<Sqlite>) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl InsertRecord for Widget {
    type ReturnType = ();
    async fn insert_record(self, _pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl DeleteRecord for Widget {
    type ReturnType = ();
    async fn delete_record(_pool: &Pool<Sqlite>, _primary_key: i64) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

/// The associated type is what the pool parameter follows, so reading it back here pins
/// the association the rest of the file relies on.
#[test]
fn the_record_names_its_database() {
    fn assert_sqlite<T: HasDatabase<Database = Sqlite>>() {}
    assert_sqlite::<Widget>();
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p sql_traits --test database_generic`
Expected: FAIL — `cannot find trait 'HasDatabase' in crate 'sql_traits'`, and `expected 'Pool<Postgres>', found 'Pool<Sqlite>'`.

- [ ] **Step 3: Add `HasDatabase` and generify the eleven traits in `lib.rs`**

Replace `use ::sqlx::PgPool;` with `use ::sqlx::Pool;`, then add the trait next to `HasPrimaryKey`:

```rust
/// Associates a type with the one database its queries run against.
///
/// The database travels as an associated type rather than a parameter on each trait, which
/// is what keeps every trait's parameter list and every axum mount site unchanged: a record
/// names its database once, here, and the thirteen pool-taking traits follow it.
///
/// Deliberately *not* a supertrait of [`HasPrimaryKey`]. The key, request-body and
/// update-fields machinery never touches a pool, so it stays database-agnostic and a
/// non-SQL consumer of those traits is unaffected.
///
/// `macros::Database` implements this from `#[macros(database = Sqlite)]`.
pub trait HasDatabase {
    type Database: ::sqlx::Database;
}
```

Then rewrite each pool-taking trait's signature. The eleven in `lib.rs`, in full:

```rust
#[async_trait]
pub trait GetLatestRecord: Sized + HasDatabase {
    async fn get_latest_record(
        pool: &Pool<<Self as HasDatabase>::Database>,
    ) -> Result<Option<Self>, sqlx::Error>;
}

#[async_trait]
pub trait GetRecord: Sized + HasPrimaryKey + HasDatabase {
    async fn get_record(
        pool: &Pool<<Self as HasDatabase>::Database>,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error>;
}

#[async_trait]
pub trait GetRecordWhere<T>: Sized + HasDatabase
where
    T: Send,
{
    async fn get_record_where(
        pool: &Pool<<Self as HasDatabase>::Database>,
        where_params: T,
    ) -> Result<Option<Self>, sqlx::Error>;
}

#[async_trait]
pub trait ListRecords: Sized + HasDatabase {
    async fn list_records(
        pool: &Pool<<Self as HasDatabase>::Database>,
    ) -> Result<Vec<Self>, sqlx::Error>;
}

#[async_trait]
pub trait ListRecordsWhere<T>: Sized + HasDatabase {
    async fn list_records_where(
        pool: &Pool<<Self as HasDatabase>::Database>,
        where_params: T,
    ) -> Result<Vec<Self>, sqlx::Error>;
}

#[async_trait]
pub trait InsertRecord: HasDatabase {
    type ReturnType: Sized;
    async fn insert_record(
        self,
        pool: &Pool<<Self as HasDatabase>::Database>,
    ) -> Result<Self::ReturnType, sqlx::Error>;
}

#[async_trait]
pub trait BulkInsertRecords: Sized + Sync + HasDatabase {
    type ReturnType: Sized;
    async fn bulk_insert_records(
        pool: &Pool<<Self as HasDatabase>::Database>,
        records: &[Self],
    ) -> Result<Self::ReturnType, sqlx::Error>;
}

#[async_trait]
pub trait ReplaceRecord: HasPrimaryKey + HasDatabase + Sized
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    async fn replace_record(
        self,
        pool: &Pool<<Self as HasDatabase>::Database>,
    ) -> Result<Option<Self>, sqlx::Error>;
}

#[async_trait]
pub trait UpdateRecord: HasUpdateFields + HasDatabase
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    async fn update_record(
        pool: &Pool<<Self as HasDatabase>::Database>,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
        update_fields: <Self as HasUpdateFields>::UpdateFields,
    ) -> Result<Option<Self>, sqlx::Error>;
}

#[async_trait]
pub trait DeleteRecord: HasPrimaryKey + HasDatabase
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    type ReturnType: Sized;
    async fn delete_record(
        pool: &Pool<<Self as HasDatabase>::Database>,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error>;
}

#[async_trait]
pub trait DeleteRecordsWhere<T>: HasDatabase
where
    T: Send,
{
    type ReturnType: Sized;
    async fn delete_records_where(
        pool: &Pool<<Self as HasDatabase>::Database>,
        where_params: T,
    ) -> Result<Self::ReturnType, sqlx::Error>;
}
```

Keep every existing doc comment on these traits exactly as it is; only the supertrait list and the pool parameter change.

- [ ] **Step 4: Generify the two paginated traits in `pagination.rs`**

Replace `use ::sqlx::PgPool;` with `use ::sqlx::Pool;`, add `use crate::HasDatabase;`, then:

```rust
#[async_trait]
pub trait ListRecordsPaginated<P>: Sized + HasDatabase
where
    P: PaginationParams + Send,
{
    async fn list_records_paginated(
        pool: &Pool<<Self as HasDatabase>::Database>,
        params: P,
    ) -> Result<Page<Self, P::Pagination>, sqlx::Error>;
}

#[async_trait]
pub trait ListRecordsWherePaginated<T, P>: Sized + HasDatabase
where
    T: Send,
    P: PaginationParams + Send,
{
    async fn list_records_where_paginated(
        pool: &Pool<<Self as HasDatabase>::Database>,
        where_params: T,
        params: P,
    ) -> Result<Page<Self, P::Pagination>, sqlx::Error>;
}
```

- [ ] **Step 5: Add the features and dev-dependencies**

In `crates/sql_traits/Cargo.toml`, add the forwarding features and the test-only driver:

```toml
# Driver selection is the consumer's, exactly as the runtime already is. These forward to
# `sqlx` so a consumer can pick a driver while still depending on this crate's `sqlx`
# re-export rather than declaring `sqlx` themselves. No default: a SQLite-only consumer
# must never be made to compile `sqlx-postgres`.
[features]
postgres = ["sqlx/postgres"]
sqlite = ["sqlx/sqlite"]
mysql = ["sqlx/mysql"]
any = ["sqlx/any"]
```

and under `[dev-dependencies]`:

```toml
# Enables the SQLite driver and a runtime for test builds only. The library picks neither,
# leaving both to the consumer; the test crates need a concrete database to name and, in
# `tests/pagination.rs`, a real in-memory one to query.
sqlx = { workspace = true, features = ["sqlite", "runtime-tokio"] }
# `tests/pagination.rs` awaits real queries against an in-memory SQLite database.
tokio = { workspace = true }
```

- [ ] **Step 6: Update the existing test fixtures**

Each of `tests/derive_macros.rs`, `tests/request_body_traits.rs`, `tests/update_fields_traits.rs` and `tests/pagination.rs` has fixtures implementing a pool-taking trait. For each fixture type:

1. Add the derive and directive to the struct:

```rust
#[derive(macros::Database)]
#[macros(database = Sqlite)]
```

(where the struct already carries a `#[derive(...)]`, add `macros::Database` to that list rather than adding a second attribute).

2. Change every `use sql_traits::sqlx::{self, PgPool};` to `use sql_traits::sqlx::{self, Pool, Sqlite};` and every `&PgPool` in a fixture signature to `&Pool<Sqlite>`.

`sql_traits` dev-dependencies enable only the SQLite driver, so these fixtures use `Sqlite`; the two-database assertion belongs to `axum_helpers` in Task 3.

- [ ] **Step 7: Run the whole crate's tests**

Run: `cargo test -p sql_traits`
Expected: PASS, including the new `database_generic` target.

- [ ] **Step 8: Lint and commit**

```bash
cargo clippy -p sql_traits --all-targets && cargo fmt --check
git add crates/sql_traits
git commit -m "Make the SQL traits generic over the database via HasDatabase"
```

---

### Task 3: The thirteen route traits

**Files:**
- Modify: `crates/axum_helpers/src/traits.rs`
- Modify: `crates/axum_helpers/Cargo.toml`
- Modify: `crates/axum_helpers/tests/derive_macros.rs`, `crates/axum_helpers/tests/route_traits.rs`, `crates/axum_helpers/tests/route_responses.rs` (fixture signatures only — `route_responses.rs` keeps its fake SQL impls until Task 5)

**Interfaces:**
- Consumes: `sql_traits::HasDatabase` (Task 2), `macros::Database` (Task 1).
- Produces: all thirteen route traits taking `State<Pool<<Self as HasDatabase>::Database>>`. Task 5 rewrites `route_responses.rs` against these.

- [ ] **Step 1: Write the failing test**

Add a second fixture to `crates/axum_helpers/tests/route_traits.rs`, on a *different* database from the existing `Gadget`, so "generic over the database" is asserted at the consumer boundary rather than claimed in a doc comment. Append:

```rust
// --- A second fixture on a second database ------------------------------------------
//
// `Gadget` above is Postgres. This one is SQLite, and the assertion is simply that both
// mount: the route traits carry no database of their own, they follow each record's
// `HasDatabase`. A regression that re-hard-coded one driver would fail to compile here.

use axum_helpers::sqlx::{Pool, Sqlite};

#[derive(axum_helpers::serde::Serialize, macros::Database)]
#[macros(database = Sqlite)]
#[serde(crate = "axum_helpers::serde")]
struct Sprocket {
    id: i64,
    name: String,
}

#[derive(axum_helpers::serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
struct SprocketFilter {
    name: String,
}

impl HasPrimaryKey for Sprocket {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl GetRecord for Sprocket {
    async fn get_record(
        _pool: &Pool<Sqlite>,
        _primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl ListRecords for Sprocket {
    async fn list_records(_pool: &Pool<Sqlite>) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl ListRecordsWhere<SprocketFilter> for Sprocket {
    async fn list_records_where(
        _pool: &Pool<Sqlite>,
        _where_params: SprocketFilter,
    ) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

impl GetRecordRoute for Sprocket {}
impl ListRecordsRoute for Sprocket {}
impl ListRecordsWhereRoute<SprocketFilter> for Sprocket {
    type PathParams = SprocketFilter;
}

/// Mounting is the check that matches the failure mode: a handler can satisfy its trait
/// bounds and still be rejected by `Router::route`. No turbofish is needed — the state
/// type pins the database.
#[test]
fn sqlite_backed_handlers_mount_on_a_router() {
    let _router: Router<Pool<Sqlite>> = Router::new()
        .route("/sprockets/{id}", get(Sprocket::get_record_route))
        .route("/sprockets", get(Sprocket::list_records_route))
        .route("/sprockets/by-name", get(Sprocket::list_records_where_route));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p axum_helpers --test route_traits`
Expected: FAIL — `expected 'Pool<Postgres>', found 'Pool<Sqlite>'` on the `GetRecord` impl.

- [ ] **Step 3: Generify the route traits**

In `crates/axum_helpers/src/traits.rs`, replace `use sqlx::PgPool;` with `use sqlx::Pool;`, add `HasDatabase` to the `use sql_traits::{...}` list, and change the state extractor on all thirteen handlers from

```rust
        State(pool): State<PgPool>,
```

to

```rust
        State(pool): State<Pool<<Self as HasDatabase>::Database>>,
```

The thirteen are `GetLatestRoute`, `GetRecordRoute`, `GetRecordWhereRoute<T>`, `ListRecordsRoute`, `ListRecordsPaginatedRoute<Q>`, `ListRecordsWhereRoute<T>`, `ListRecordsWherePaginatedRoute<T, Q>`, `CreateRoute<'de>`, `BulkCreateRoute<'de>`, `DeleteRoute`, `DeleteRecordsWhereRoute<T>`, `ReplaceRoute`, `UpdateRoute`.

No route trait needs `HasDatabase` added to its supertrait list — each already has a SQL supertrait that carries it. Handler bodies, `type PathParams`, and every `where` clause are unchanged.

- [ ] **Step 4: Add the forwarding features**

In `crates/axum_helpers/Cargo.toml`:

```toml
# Forwarded to `sql_traits` and `sqlx` together, so a consumer enabling a driver here gets
# it in both. No default, for the same reason `sql_traits` has none.
[features]
postgres = ["sql_traits/postgres", "sqlx/postgres"]
sqlite = ["sql_traits/sqlite", "sqlx/sqlite"]
mysql = ["sql_traits/mysql", "sqlx/mysql"]
any = ["sql_traits/any", "sqlx/any"]
```

and extend the existing `sqlx` dev-dependency, keeping its comment and adding to it:

```toml
sqlx = { workspace = true, features = ["postgres", "sqlite", "runtime-tokio"] }
```

Both drivers, because `tests/route_traits.rs` now carries a fixture on each.

- [ ] **Step 5: Update the existing fixtures**

In `tests/derive_macros.rs`, `tests/route_traits.rs` and `tests/route_responses.rs`, every fixture record needs a database. Add `macros::Database` to each fixture's existing `#[derive(...)]` list and `#[macros(database = Postgres)]` alongside its other `#[macros(...)]` attributes. Their `&PgPool` signatures stay exactly as they are — `Pool<Postgres>` *is* `PgPool`, so they already satisfy the new signature.

`route_responses.rs` keeps its fake SQL impls *and* takes `#[macros(database = Postgres)]`
in this task, because its fixture signatures are still `&PgPool`. Task 5 switches that fixture
to `Sqlite` when it rewrites the file against a real database.

- [ ] **Step 6: Run the whole workspace**

Run: `cargo test`
Expected: PASS across all four crates.

- [ ] **Step 7: Lint and commit**

```bash
cargo clippy --all-targets && cargo fmt --check
git add crates/axum_helpers
git commit -m "Make the route traits follow each record's database"
```

---

### Task 4: Remove the hard-coded workspace driver

This is the task that delivers the goal "a SQLite-only consumer never compiles `sqlx-postgres`". Until now the workspace `sqlx` dependency has been forcing `postgres` on for everything.

**Files:**
- Modify: `Cargo.toml` (workspace root)

**Interfaces:**
- Consumes: the forwarding features from Tasks 2 and 3.
- Produces: a workspace where neither library crate enables a driver.

- [ ] **Step 1: Write the failing check**

There is no test target for this; the assertion is a build. Run it first and watch it *pass for the wrong reason* — the driver is currently forced on, so this proves nothing yet:

Run: `cargo build -p sql_traits --no-default-features`
Expected: PASS, but `cargo tree -p sql_traits -i sqlx-postgres` still lists `sqlx-postgres`, which is the defect.

- [ ] **Step 2: Drop the driver from the workspace dependency**

In the root `Cargo.toml`:

```toml
sqlx = { version = "0.9" }
```

- [ ] **Step 3: Verify no driver is pulled in**

Run: `cargo tree -p sql_traits -i sqlx-postgres`
Expected: an error or empty result — nothing depends on `sqlx-postgres` when no feature asks for it.

Run: `cargo build -p sql_traits --features sqlite`
Expected: PASS.

Run: `cargo tree -p sql_traits -i sqlx-postgres --features sqlite`
Expected: still nothing — enabling SQLite does not drag Postgres in.

- [ ] **Step 4: Verify the workspace still tests green**

Run: `cargo test`
Expected: PASS. The test crates enable their own drivers through dev-dependencies, which is why dropping the workspace feature does not break them.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml
git commit -m "Stop forcing the postgres driver on every crate in the workspace"
```

---

### Task 5: `route_responses.rs` against real in-memory SQLite

The largest single piece. Every SQL impl in this file is currently a fake that ignores the pool, while the file's subject is what handlers actually answer — so real tables and real queries strengthen it directly.

**Files:**
- Modify: `crates/axum_helpers/tests/route_responses.rs` (709 lines)

**Interfaces:**
- Consumes: the generic route traits (Task 3), the SQLite dev-dependency (Task 3).
- Produces: no new public API. A `pool()` helper other tests in the file share.

- [ ] **Step 1: Write the failing test**

First switch the file's fixture over to SQLite: change `use axum_helpers::sqlx::{self, PgPool};`
to `use axum_helpers::sqlx::{self, Pool, Sqlite};`, change the `Widget` fixture's attribute from
`#[macros(database = Postgres)]` (set in Task 3) to `#[macros(database = Sqlite)]`, and change
every `&PgPool` in the file to `&Pool<Sqlite>`.

Then replace the file's `pool()` helper and add a first real-query test. The existing helper
builds a `PgPool` with `connect_lazy` and is never connected to; the new one is a real database:

```rust
/// A real in-memory SQLite database with this file's schema already created.
///
/// `sqlx` shares one in-memory database across a pool's connections, so no
/// `max_connections(1)` or shared-cache URI is needed — a table created here is visible to
/// every connection the pool hands out. Each call builds a fresh database, so tests cannot
/// see one another's rows.
async fn pool() -> Pool<Sqlite> {
    let pool = Pool::<Sqlite>::connect("sqlite::memory:")
        .await
        .expect("an in-memory database");
    sqlx::query("CREATE TABLE widget (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .expect("the schema");
    pool
}

/// Inserts one row and returns its key, for tests that need a record to address.
async fn insert_widget(pool: &Pool<Sqlite>, name: &str) -> i64 {
    sqlx::query_as::<_, (i64,)>("INSERT INTO widget (name) VALUES (?) RETURNING id")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("the insert")
        .0
}

#[tokio::test]
async fn get_answers_200_with_the_row_that_is_actually_stored() {
    let pool = pool().await;
    let id = insert_widget(&pool, "cog").await;

    let response = Widget::get_record_route(State(pool), Path(id)).await;

    assert_eq!(response.status(), StatusCode::OK);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p axum_helpers --test route_responses get_answers_200_with_the_row`
Expected: FAIL — the fixture's `GetRecord` impl still returns `Ok(None)` regardless of the pool, so the handler answers `404`.

- [ ] **Step 3: Convert the fixture's SQL impls to real queries**

Replace each fake impl on `Widget`. Row mapping goes through a tuple rather than `sqlx::FromRow`, which keeps the test crate free of an extra `sqlx` feature:

```rust
#[async_trait]
impl GetRecord for Widget {
    async fn get_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM widget WHERE id = ?")
            .bind(primary_key)
            .fetch_optional(pool)
            .await
            .map(|row| row.map(|(id, name)| Widget { id, name }))
    }
}

#[async_trait]
impl ReplaceRecord for Widget {
    async fn replace_record(self, pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, (i64, String)>(
            "UPDATE widget SET name = ? WHERE id = ? RETURNING id, name",
        )
        .bind(&self.name)
        .bind(self.id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(|(id, name)| Widget { id, name }))
    }
}

#[async_trait]
impl UpdateRecord for Widget {
    async fn update_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
        update_fields: WidgetUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        // Fetch-apply-replace, which is one of the two shapes `UpdateRecord`'s own docs
        // describe. It keeps the statement static, and `UpdateRoute` has already rejected
        // an empty update with `400` before this is reached.
        let Some(record) = Self::get_record(pool, primary_key).await? else {
            return Ok(None);
        };
        update_fields.apply(record).replace_record(pool).await
    }
}
```

`UPDATE ... RETURNING` is supported by the SQLite version `sqlx` builds against, which is what lets a missing row come back as `Ok(None)` rather than an error — the convention these traits require so a route can answer `404`.

- [ ] **Step 4: Run the new test**

Run: `cargo test -p axum_helpers --test route_responses get_answers_200_with_the_row`
Expected: PASS.

- [ ] **Step 5: Convert the remaining tests in the file**

Work through the file's other tests. Each one changes in the same two ways, and no assertion changes:

1. `let pool = pool().await;` replaces the old synchronous `pool()` call, and the value is moved into `State(pool)`.
2. A test that addresses an existing record calls `insert_widget(&pool, "...").await` first and uses the returned key, instead of the hard-coded `1`.

`MISSING_ID` keeps its meaning and needs no seeding: a key with no row inserted for it genuinely matches nothing now, which is what the `404` tests want. Tests that never reach the database — the empty-update `400`, which `UpdateRoute` rejects before calling `update_record` — need only the `pool().await` change.

- [ ] **Step 6: Run the whole file**

Run: `cargo test -p axum_helpers --test route_responses`
Expected: PASS, all 25 tests.

- [ ] **Step 7: Lint and commit**

```bash
cargo clippy -p axum_helpers --all-targets && cargo fmt --check
git add crates/axum_helpers/tests/route_responses.rs
git commit -m "Drive the route response tests against a real in-memory SQLite database"
```

---

### Task 6: Real paginated queries in `sql_traits/tests/pagination.rs`

The cursor contract is a set of promises no fake impl can break: a total order, a `limit + 1`
fetch, an opt-in total. Only a real database can hold an implementation to them. This task
deliberately starts from the naive cursor implementation so the test that catches it is seen
failing — that failure *is* the reason the contract exists.

**Files:**
- Modify: `crates/sql_traits/tests/pagination.rs`

**Interfaces:**
- Consumes: `ListRecordsPaginated` (Task 2), the SQLite and `tokio` dev-dependencies (Task 2).
- Produces: no new public API.

- [ ] **Step 1: Add the fixture and the offset implementation**

The existing wire-format tests in this file stay exactly as they are. The file already has
`use sql_traits::{CursorPagination, OffsetPagination, Page};` — **extend that existing line**
rather than adding a second one, so it reads:

```rust
use sql_traits::{
    CursorPagination, CursorParams, ListRecordsPaginated, OffsetPagination, OffsetParams, Page,
};
```

and add two new use lines beside it:

```rust
use sql_traits::async_trait::async_trait;
use sql_traits::sqlx::{self, Pool, Sqlite};
```

Then append the fixture:

```rust
// --- Real paginated queries ------------------------------------------------------------
//
// The wire-format tests above pin the envelope's shape. These pin the contract
// `ListRecordsPaginated`'s documentation states and no fake impl can be held to: a cursor
// needs a total order, another page is detected by fetching `limit + 1`, and `total` is
// filled if and only if it was asked for.

#[derive(macros::Database)]
#[macros(database = Sqlite)]
struct Entry {
    id: i64,
}

async fn seeded_pool(rows: i64) -> Pool<Sqlite> {
    let pool = Pool::<Sqlite>::connect("sqlite::memory:")
        .await
        .expect("an in-memory database");
    sqlx::query("CREATE TABLE entry (id INTEGER PRIMARY KEY)")
        .execute(&pool)
        .await
        .expect("the schema");
    for id in 1..=rows {
        sqlx::query("INSERT INTO entry (id) VALUES (?)")
            .bind(id)
            .execute(&pool)
            .await
            .expect("a seeded row");
    }
    pool
}

#[async_trait]
impl ListRecordsPaginated<OffsetParams> for Entry {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        let rows =
            sqlx::query_as::<_, (i64,)>("SELECT id FROM entry ORDER BY id LIMIT ? OFFSET ?")
                .bind(params.limit as i64)
                .bind(params.offset as i64)
                .fetch_all(pool)
                .await?;

        // Filled if and only if the caller asked, because it costs a count of the whole set.
        let total = if params.include_total {
            let (count,) = sqlx::query_as::<_, (i64,)>("SELECT count(*) FROM entry")
                .fetch_one(pool)
                .await?;
            Some(count as u32)
        } else {
            None
        };

        Ok(Page {
            data: rows.into_iter().map(|(id,)| Entry { id }).collect(),
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total,
            },
        })
    }
}
```

- [ ] **Step 2: Add the naive cursor implementation**

Write it the obvious, wrong way first — fetch exactly `limit` rows and report the last one as
the next cursor:

```rust
#[async_trait]
impl ListRecordsPaginated<CursorParams<i64>> for Entry {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: CursorParams<i64>,
    ) -> Result<Page<Self, CursorPagination<i64>>, sqlx::Error> {
        let rows =
            sqlx::query_as::<_, (i64,)>("SELECT id FROM entry WHERE id > ? ORDER BY id LIMIT ?")
                .bind(params.cursor.unwrap_or(0))
                .bind(params.limit as i64)
                .fetch_all(pool)
                .await?;

        let next = rows.last().map(|(id,)| *id);

        Ok(Page {
            data: rows.into_iter().map(|(id,)| Entry { id }).collect(),
            pagination: CursorPagination {
                limit: params.limit,
                next,
            },
        })
    }
}
```

- [ ] **Step 3: Write the tests**

```rust
#[tokio::test]
async fn an_unrequested_total_is_not_counted() {
    let pool = seeded_pool(10).await;
    let params = OffsetParams { limit: 3, offset: 0, include_total: false };

    let page = <Entry as ListRecordsPaginated<OffsetParams>>::list_records_paginated(&pool, params)
        .await
        .expect("the page");

    assert_eq!(page.data.len(), 3);
    assert_eq!(page.pagination.total, None, "a total nobody asked for must not be filled");
}

#[tokio::test]
async fn a_requested_total_counts_the_whole_set_not_the_page() {
    let pool = seeded_pool(10).await;
    let params = OffsetParams { limit: 3, offset: 0, include_total: true };

    let page = <Entry as ListRecordsPaginated<OffsetParams>>::list_records_paginated(&pool, params)
        .await
        .expect("the page");

    assert_eq!(page.data.len(), 3, "the page is still a page");
    assert_eq!(page.pagination.total, Some(10));
}

/// A page that is not full is the last page, and must say so. Reporting a next cursor here
/// costs the caller an extra round trip to discover an empty page.
#[tokio::test]
async fn the_last_page_reports_no_next_cursor() {
    let pool = seeded_pool(4).await;
    let params = CursorParams { limit: 10, cursor: None };

    let page =
        <Entry as ListRecordsPaginated<CursorParams<i64>>>::list_records_paginated(&pool, params)
            .await
            .expect("the page");

    assert_eq!(page.data.len(), 4);
    assert_eq!(page.pagination.next, None, "there is no page after the last one");
}

/// The promise that makes cursor pagination worth having: walking every page visits each
/// row exactly once, with nothing skipped and nothing repeated.
#[tokio::test]
async fn walking_the_cursor_visits_every_row_exactly_once() {
    let pool = seeded_pool(10).await;
    let mut seen = Vec::new();
    let mut cursor = None;

    loop {
        let params = CursorParams { limit: 3, cursor };
        let page =
            <Entry as ListRecordsPaginated<CursorParams<i64>>>::list_records_paginated(&pool, params)
                .await
                .expect("the page");
        seen.extend(page.data.iter().map(|entry| entry.id));
        match page.pagination.next {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }

    assert_eq!(seen, (1..=10).collect::<Vec<i64>>());
}
```

- [ ] **Step 4: Run the tests and watch the cursor contract fail**

Run: `cargo test -p sql_traits --test pagination`
Expected: `the_last_page_reports_no_next_cursor` FAILS with
`assertion `left == right` failed: there is no page after the last one` — the naive
implementation cannot tell a full page from a final one, so it reports `Some(4)`.

The other three pass, including `walking_the_cursor_visits_every_row_exactly_once`: the naive
implementation still visits every row, it just pays an extra round trip to find an empty page.
That is exactly why the last-page test is the one that has to exist.

- [ ] **Step 5: Fetch `limit + 1` so a final page is distinguishable**

Replace the body of the cursor impl from Step 2:

```rust
#[async_trait]
impl ListRecordsPaginated<CursorParams<i64>> for Entry {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: CursorParams<i64>,
    ) -> Result<Page<Self, CursorPagination<i64>>, sqlx::Error> {
        // Fetch one more than asked for: the extra row is how another page is detected
        // without paying for a second count. `id` is the primary key, so the sort is
        // total and no row can fall on both sides of a page boundary.
        let mut rows =
            sqlx::query_as::<_, (i64,)>("SELECT id FROM entry WHERE id > ? ORDER BY id LIMIT ?")
                .bind(params.cursor.unwrap_or(0))
                .bind(params.limit as i64 + 1)
                .fetch_all(pool)
                .await?;

        let next = if rows.len() > params.limit as usize {
            rows.pop();
            rows.last().map(|(id,)| *id)
        } else {
            None
        };

        Ok(Page {
            data: rows.into_iter().map(|(id,)| Entry { id }).collect(),
            pagination: CursorPagination {
                limit: params.limit,
                next,
            },
        })
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p sql_traits --test pagination`
Expected: PASS — the four new tests plus the existing wire-format tests.

- [ ] **Step 7: Lint and commit**

```bash
cargo clippy -p sql_traits --all-targets && cargo fmt --check
git add crates/sql_traits/tests/pagination.rs
git commit -m "Assert the pagination contract against a real SQLite database"
```

---

### Task 7: Documentation and changelog

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `CLAUDE.md` (root)
- Modify: `crates/sql_traits/CLAUDE.md`
- Modify: `crates/axum_helpers/CLAUDE.md`
- Modify: `crates/macros/CLAUDE.md`

**Interfaces:**
- Consumes: everything from Tasks 1-6.
- Produces: no code.

- [ ] **Step 1: Write the changelog entries**

Under `## Unreleased` in `CHANGELOG.md`, matching the existing long-form style — what changed, why, and what a consumer does about it:

```markdown
### Added

- `sql_traits::HasDatabase`, associating a record with the one database its queries run
  against. The database travels as an associated type rather than a parameter on each
  trait, which is what keeps every trait's parameter list and every axum mount site
  unchanged: a record names its database once and the thirteen pool-taking traits follow
  it. It is deliberately not a supertrait of `HasPrimaryKey` — the key, request-body and
  update-fields machinery never touches a pool and stays database-agnostic.
- `macros::Database`, a derive reading `#[macros(database = Sqlite)]` and emitting the
  `HasDatabase` impl. Standalone rather than folded into `Record`, so a hand-written record
  and a separate insert-side type can both use it. The directive is required; its absence
  is a compile error naming the accepted set (`Postgres`, `Sqlite`, `MySql`, `Any`) rather
  than a silent default.
- Driver features on `sql_traits` and `axum_helpers` (`postgres`, `sqlite`, `mysql`,
  `any`), forwarding to `sqlx`. They exist so a consumer can pick a driver while still
  depending on the re-exported `sqlx` rather than declaring it themselves.

### Changed

- **Breaking.** All thirteen pool-taking SQL traits and all thirteen route traits now take
  `Pool<<Self as HasDatabase>::Database>` in place of `PgPool`, and every type implementing
  one must implement `HasDatabase`. Add `#[derive(macros::Database)]` with
  `#[macros(database = Postgres)]` to each record, or write the three-line impl by hand.
  Existing method bodies and signatures do not change: `Pool<Postgres>` *is* `PgPool`, so an
  impl written `async fn get_record(pool: &PgPool, ...)` still satisfies the new signature
  once its `Database` is `Postgres`.
- **Breaking.** Neither crate enables a `sqlx` driver any more, so a consumer must enable
  one: `sql_traits = { ..., features = ["postgres"] }`, and likewise for `axum_helpers`.
  Without it, nothing names a driver and `sqlx::Postgres` will not resolve. Note that cargo
  feature unification can mask this — if any other crate in your graph enables
  `sqlx/postgres`, the build may succeed until that dependency changes. Set the feature
  explicitly rather than relying on it. The upside is that a SQLite-only consumer no longer
  compiles `sqlx-postgres` at all.
```

- [ ] **Step 2: Update the root `CLAUDE.md`**

In the crates map section, note that the SQL and route traits are generic over the database and that `sql_traits` picks neither a driver nor a runtime feature. Add `database = Db` to any listing of the `#[macros(...)]` directives.

- [ ] **Step 3: Update `crates/sql_traits/CLAUDE.md`**

Add `HasDatabase` to the trait overview: what it is, that the database is an associated type so parameter lists do not grow, and why it is separate from `HasPrimaryKey`. Note that the crate enables no driver feature and that the forwarding features exist for consumers.

- [ ] **Step 4: Update `crates/axum_helpers/CLAUDE.md`**

Note that a handler's state type follows the record's database — `State<Pool<Self::Database>>` — so a mount site needs no turbofish, and that the crate's driver features forward to `sql_traits`.

- [ ] **Step 5: Update `crates/macros/CLAUDE.md`**

Document the `Database` derive, the `database` directive, the accepted ident set, that the directive is required rather than defaulted, and that `Record`/`Update`/`PrimaryKey` tolerate it without consuming it — the same tolerate-don't-consume rule that already lets `Record` and `Update` share a struct.

- [ ] **Step 6: Verify and commit**

Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
Expected: PASS.

```bash
git add CHANGELOG.md CLAUDE.md crates/sql_traits/CLAUDE.md crates/axum_helpers/CLAUDE.md crates/macros/CLAUDE.md
git commit -m "Document the database-generic traits"
```

---

## Execution Order and Rationale

1. **Task 1 (macros)** first because `macros` depends on none of the other crates and its tests assert on token strings, so the derive can be built and fully tested before `HasDatabase` exists. Every later task's fixtures then use the derive rather than hand-writing impls and replacing them.
2. **Task 2 (sql_traits)** before **Task 3 (axum_helpers)**, following the one-way layering.
3. **Task 4 (workspace driver)** after both library crates are generic, because dropping the forced `postgres` feature is only safe once neither crate names a concrete driver.
4. **Tasks 5 and 6 (real SQLite)** last among the code tasks: they are test rewrites that depend on everything above, and Task 5 is the largest single piece in the plan.
5. **Task 7 (docs)** last, so the changelog describes what actually landed.
