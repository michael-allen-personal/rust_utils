# Changelog

## v0.6.0

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
