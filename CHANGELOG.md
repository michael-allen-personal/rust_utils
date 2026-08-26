# Changelog

## v0.7.0

### Added

- `sql_traits::GetRecord`: fetches a single record by its `HasPrimaryKey::PrimaryKey`, distinct from the filter-based lookup that is now `GetRecordWhere`
- `axum_helpers::GetRecordRoute` and a matching `GetRecordRoute` derive: an axum handler that reads the primary key out of the URL path and returns the record
- `#[derive(BasicCrudRoutes)]` now also emits `impl GetRecordRoute`
- Unit tests for the derive expansion functions in `macros`, covering the `PrimaryKey` single/composite/error cases and the emitted route impls
- `sql_traits::DeleteRecordsWhere<T>`: deletes every record matching a filter, returning an implementor-chosen `ReturnType` (a row count, the deleted rows, or `()`). This is the filter-based counterpart to `DeleteSQL`, which addresses a single record by primary key
- `axum_helpers::DeleteRecordsWhereRoute<T>`: an axum handler that reads the filter out of the URL path and returns `200 OK` with `DeleteRecordsWhere::ReturnType` as JSON. `DeleteRoute` discards its return value and answers `204 No Content`; this one serializes it, so a caller can see what the delete actually matched. Like the other `*Where` route traits it has no derive macro, because `PathParams` has to be chosen by the implementor rather than inferred from the struct
- A route-mounting compile test (`crates/axum_helpers/tests/route_traits.rs`) that puts every route handler — including the three `*Where` handlers, which no test previously touched — onto an `axum::Router`. Implementing a route trait and mounting it are separate checks: `Router::route` is where axum's `Handler` requirements are actually enforced, and it is the site where the pre-`0.7.0` missing-`DeserializeOwned` bug surfaced. Like the derive tests, it reaches `axum`/`serde`/`sqlx`/`sql_traits` only through `axum_helpers`' re-exports

### Changed

- **Breaking.** `sql_traits::GetRecord<T>` (filter-based) is renamed to `GetRecordWhere<T>` and its method `get_record` to `get_record_where`. The `GetRecord` name now means the primary-key lookup, so a stale `impl GetRecord<MyFilter>` fails on arity rather than silently binding to the new trait
- **Breaking.** `axum_helpers::GetRoute<T>` is renamed to `GetRecordWhereRoute<T>` and its method `get_route` to `get_record_where_route`, matching the `<SqlTraitName>Route` convention. The `GetRoute` name is retired rather than reused, so existing call sites fail with "cannot find trait" instead of silently dispatching to the new primary-key handler
- **Breaking.** `#[derive(BasicCrudRoutes)]` requires implementing types to also implement `GetRecord`, since `GetRecordRoute` takes it as a supertrait. Types deriving it today need a `GetRecord` impl added
- **Breaking.** Route traits that extract from the URL path now require the extracted type to be `DeserializeOwned`: `GetRecordRoute` and `DeleteRoute` on `HasPrimaryKey::PrimaryKey`, `GetRecordWhereRoute` and `ListRecordsWhereRoute` on `PathParams`. Previously such a type could implement the trait and only fail when the handler was passed to `Router::route`, with an opaque `Handler` trait error that never mentioned `Deserialize`. No route that could actually be mounted before is affected. Generic code over these traits must restate the bound; concrete impls, including everything the derives emit, get it checked at the impl site
- **Breaking.** `GetRecordRoute` and `GetRecordWhereRoute` return `404 Not Found` rather than `204 No Content` when no record matches, via the existing `ApiError::NotFoundError` path. `204` is a 2xx, so callers could not distinguish a missing record from a successful empty fetch. `GetLatestRoute` still returns `204`, where an empty table is not a missing resource
- The route derive macros and `BasicCrudRoutes` are generated from shared emitters rather than duplicating each impl body, so the standalone derives and the bundle cannot drift
- `proc_macro::TokenStream` now appears only in the `#[proc_macro_derive]` signatures; every expansion function takes and returns `proc_macro2::TokenStream`, which is what makes them unit-testable. `proc-macro2` is now a direct dependency of `macros` (it was already present transitively via `syn`/`quote`)
- The fetch-one route handlers share one `optional_record_response` helper, making the not-found status a single decision instead of three copies

## v0.6.0

### Changed

- Bumped `sqlx` from `0.8` to `0.9`. This is a breaking change for downstream crates: `0.8` and `0.9` are not compatible and do not unify, so consumers must move to `sqlx` `0.9` (or depend on it through this crate's re-export)

### Removed

- `MaxVecCapacity` derive macro. It emitted an `impl common_parser::MaxVecCapacity` block referencing the `common_parser` crate, which is not a dependency anywhere in the workspace, so the derive could never compile for any consumer

## v0.5.1

### Added

- `sql_traits` now re-exports `async_trait` and `sqlx`; `axum_helpers` now re-exports `async_trait`, `axum`, `serde`, `serde_json`, `sql_traits`, and `sqlx`. Downstream crates can depend on these through the re-exports to guarantee a single compiled copy of each, avoiding duplicate-crate type mismatches (e.g. two incompatible `sqlx::Pool` types)
- Consumer-perspective compile tests for the derive macros in `sql_traits` and `axum_helpers`. Each test crate depends only on the crate under test plus `macros` — with no direct `serde`/`sqlx`/`sql_traits` dependency — so a successful build proves the generated code is self-contained

### Changed

- Derive macros now emit absolute paths routed through the re-exports (e.g. `::axum_helpers::sql_traits::InsertSQL`, `::axum_helpers::serde::Serialize`) instead of bare `sql_traits::`/`serde::` paths, so generated code resolves without the use site declaring those dependencies by name
- Loosened workspace dependency requirements from exact patch pins (e.g. `0.1.89`) to minor-level constraints (e.g. `0.1`)

## v0.5.0

### Changed

- `Result` type alias in `axum_helpers` now takes an optional error type parameter (`Result<T, E = ApiError>`), so both `Result<T>` and `Result<T, E>` resolve to one alias within the same file ([#1](https://github.com/michael-allen-personal/rust_utils/issues/1))

## v0.4.0

### Added

- `GetRecord` and `ListRecordsWhere` SQL traits for filtered queries
- `GetRoute` and `ListRecordsWhereRoute` axum route traits

### Changed

- `BulkInsertSQL` traits now have an associated `ReturnType` specified by the implementor
- `BulkCreateRoute` trait now returns the associated `BulkInsertSQL::ReturnType` in the response instead of an empty body
- `CreateRoute`, `BulkCreateRoute`, and `BasicCrudRoutes` derive macros now include `ReturnType: Serialize` where clauses
- Cleaned up docstrings on SQL traits

## v0.3.0

### Changed

- `CreateRoute` trait now returns the associated `InsertSQL::ReturnType` in the response instead of an empty body

## v0.2.0

### Added

- `ListRecordsRoute` trait in `axum_helpers` for listing all records via a GET endpoint
- `ListRecordsRoute` derive macro in `macros`
- `ListRecordsRoute` included in `BasicCrudRoutes` derive macro

### Changed

- SQL traits and axum route handlers now take `&PgPool` instead of a generic `PgExecutor`

### Bugfixes

- Derive macros now reference `axum_helpers::` instead of `common_api::`
