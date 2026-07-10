# Changelog

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
