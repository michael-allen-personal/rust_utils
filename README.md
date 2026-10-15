# rust_utils

Shared Rust building blocks for database-backed axum services: database traits over `sqlx`,
axum route handlers built on those traits, and a few general-purpose helpers. You implement
the SQL; the derives and route traits supply the HTTP plumbing around it.

## Crates

| Crate | What it provides |
|---|---|
| `sql_traits` | Database traits over `sqlx`, generic over the driver (`GetRecord`, `ListRecords`, `InsertRecord`, `ReplaceRecord`, `UpdateRecord`, `DeleteRecord`, the `*Filtered` and paginated variants), plus the `PrimaryKey`, `Record`, `Update` and `Database` derives. Knows nothing about HTTP. |
| `axum_helpers` | An axum route handler for each SQL trait (`GetRecordRoute`, `CreateRoute`, `UpdateRoute`, …), the `BasicCrudRoutes` bundle derive, `ApiError`, and the `PathAndQuery` extractor. |
| `generic_helpers` | The `str_enum!` macro, file helpers, and the `MaxVecCapacity` derive. Standalone. |

The `*_macros` crates are private implementation details. Their derives are re-exported from
the crates above, so you never depend on them directly.

## Installation

Depend on the crates by git tag, keeping them all on the same tag:

```toml
[dependencies]
axum_helpers = { git = "https://github.com/michael-allen-personal/rust_utils", tag = "v1.0.0", features = ["postgres"] }
sql_traits = { git = "https://github.com/michael-allen-personal/rust_utils", tag = "v1.0.0" }
# Only to choose the sqlx runtime; use sqlx's types through `axum_helpers::sqlx`.
sqlx = { version = "0.9", features = ["runtime-tokio"] }
```

- **Driver.** Neither library crate enables a `sqlx` driver. Choose one with the `postgres`,
  `sqlite`, `mysql` or `any` feature on `axum_helpers` (which passes it on to `sql_traits`),
  or on `sql_traits` if that is the only crate you use.
- **Re-exports.** `axum`, `serde`, `sqlx`, `sql_traits` and `async_trait` are re-exported
  from `axum_helpers` (and `serde`, `sqlx`, `async_trait` from `sql_traits`). Use them
  through the re-exports so your service compiles against exactly one copy of each.
- **Why `sql_traits` is listed directly.** The derives' generated code refers to
  `::sql_traits::…`, which only resolves when your crate depends on `sql_traits` by name.

## Example

A record with a full set of CRUD routes. You write the SQL trait impls; the derives generate
the request-body and partial-update types and the route handlers.

```rust
use axum_helpers::axum::{Router, routing::get};
use axum_helpers::serde;
use axum_helpers::sqlx::{Pool, Postgres};
use axum_helpers::{CreateRoute, GetRecordRoute, ListRecordsRoute, UpdateRoute, DeleteRoute};

#[derive(
    serde::Serialize,
    serde::Deserialize,
    sql_traits::Database,
    sql_traits::Record,
    sql_traits::Update,
    axum_helpers::BasicCrudRoutes,
)]
#[sql_traits(database = Postgres)]
#[sql_traits(body_derive(serde::Serialize, serde::Deserialize))]
#[sql_traits(update_derive(serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
pub struct Author {
    #[sql_traits(primary_key)]
    pub id: i64,
    pub name: String,
    pub bio: Option<String>,
}

// impl GetRecord, ListRecords, InsertRecord, BulkInsertRecords,
//      ReplaceRecord, UpdateRecord and DeleteRecord for Author { ... }

pub fn routes() -> Router<Pool<Postgres>> {
    Router::new()
        .route("/authors", get(Author::list_records_route).post(Author::create_route))
        .route(
            "/authors/{id}",
            get(Author::get_record_route)
                .patch(Author::update_route)
                .delete(Author::delete_route),
        )
}
```

`examples/sample_api` is a complete, runnable version of this. It covers every SQL and route
trait, both pagination modes, query-string filters and a composite primary key, all against
an in-memory SQLite database:

```bash
cargo run -p sample_api   # serves http://127.0.0.1:3114
```

The requests in `examples/sample_api/requests.http` exercise each route.

## Development

```bash
cargo test                    # unit, integration and doc tests for the whole workspace
cargo clippy --all-targets
cargo fmt --check
```

Run `cargo test` rather than relying on `cargo check`: most assertions about generated macro
code live in the `tests/` crates, which `cargo check` does not build. No database server is
needed; tests that run real queries use in-memory SQLite.

## Versioning

Releases are tagged `vX.Y.Z` across the whole workspace, and every crate's version matches
the tag. See [CHANGELOG.md](CHANGELOG.md) for what changed in each release, including
upgrade steps for breaking changes.
