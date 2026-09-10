# Changelog

## Unreleased

### Added

- `generic_helpers` crate, moved here from the `data-monorepo` repo (where it was
  `common_parser`). Holds the `str_enum!` macro, `ParsingError`, and the
  `buf_reader_from_path` / `MaxVecCapacity` file helpers. One dependency (`error_set`)
- `MaxVecCapacity` derive macro, re-added now that the trait lives in this workspace.
  It emits `impl ::generic_helpers::MaxVecCapacity for T {}` — an absolute path through
  `generic_helpers`' root re-export, unlike the removed version's bare `common_parser::`
  path. `generic_helpers/tests/derive_macros.rs` is a consumer-perspective compile test
  that fails if the emitted path stops resolving from outside the crate
- `str_enum!` generates `TryFrom<&str>` and `TryFrom<String>` alongside `FromStr`, so the
  enums can be used from `#[serde(try_from = "String")]` and other `TryFrom`-bounded
  positions that cannot reach `FromStr`. All three route through the same private
  `accepts` matching, so aliases and case-insensitivity behave identically; the `String`
  overload moves the input into the error on failure instead of allocating a second copy
  of it
- `sql_traits::HasRequestBody` and `sql_traits::RequestBody`: a pair of traits associating
  a record with the request-body type holding every field except its primary key. The
  record side is what route traits bind on; the body side makes the assembly reachable from
  generic code holding only a body. `RequestBody::Record` is bound
  `HasRequestBody<RequestBody = Self>`, so the pairing is mutual — a body can only name a
  record that names it back, and the two sides cannot drift onto different types. That
  bound also makes `with_key` a provided method: an implementor writes nothing but the
  associated type
- `#[derive(macros::Record)]`: a superset of `PrimaryKey` that also generates `{Name}Body`
  from the fields *not* marked `#[macros(primary_key)]`, both association impls, and both
  `From` conversions — `From<(PrimaryKey, Body)> for Record` and `From<Record> for Body`.
  Together those two make `record -> (key, body) -> record` expressible as a single
  round-trip assertion, which is the test that catches a field reassembled into the wrong
  slot. Name the body type's derives with `#[macros(body_derive(...))]`: a derive macro
  cannot see sibling `#[derive(...)]` attributes, since rustc strips them before it runs.
  Every other attribute forwards to the body automatically, which is what keeps `serde` and
  `ts-rs` renames from drifting between a record and its body. Named-field structs only.
  A `#[macros(...)]` attribute that is not one of those two directives — a typo, a stray
  comma, `body_derive` in a non-list form, or a directive on the wrong item — is a
  `compile_error!` naming what was found and what is accepted, rather than being ignored.
  Silence there was dangerous: a malformed `#[macros(primary_key,)]` used to demote the
  field out of the key and *into* the generated body, leaking the key into the request body
- `sql_traits::HasUpdateFields` and `sql_traits::UpdateFields`: a pair of traits associating
  a record with the type holding a *partial* set of its non-key fields, mirroring
  `HasRequestBody`/`RequestBody` exactly. `UpdateFields::Record` is bound
  `HasUpdateFields<UpdateFields = Self>`, so the pairing is mutual and cannot drift, and
  that bound makes `apply` a provided method — an implementor writes the associated type
  and `is_empty`, nothing more. `apply` consumes the record and returns the updated one
  rather than taking `&mut`, which keeps it chainable (`fields.apply(record)`) and matches
  every other trait in the crate; none of them takes a mutable reference
- `sql_traits::double_option`: the `deserialize_with` helper that keeps the three states of
  a partial update apart over JSON. A plain derive on `Option<Option<T>>` collapses an
  explicit `null` into the same `None` an absent key produces, which would silently turn
  "clear this column" into "leave it alone". Public only because generated code has to name
  it. `serde` is consequently a dependency of `sql_traits`, and is re-exported alongside
  `sqlx` and `async_trait`
- `#[derive(macros::Update)]`: generates `{Name}Update` from the fields *not* marked
  `#[macros(primary_key)]`, each wrapped in one more `Option` than the record has — `String`
  becomes `Option<String>`, `Option<i32>` becomes `Option<Option<i32>>` — plus both
  association impls. Name its derives with `#[macros(update_derive(...))]`, the counterpart
  to `body_derive`. A field whose type is syntactically `Option<..>` also gets
  `#[serde(default, deserialize_with = "::sql_traits::double_option")]`, which is what makes
  an explicit `null` clear the column. A type *alias* for `Option<T>` cannot be recognized —
  no proc macro can resolve one — and such a field simply loses the ability to be cleared;
  it is never a type error, because the generated field type is correct either way.
  Designed to sit alongside `#[derive(macros::Record)]`, which is what supplies the
  `HasPrimaryKey` impl that `HasUpdateFields` requires
- `axum_helpers::UpdateRoute` and a matching derive: a `PATCH` handler taking the primary key
  from the URL path and a partial body from JSON, returning `200 OK` with the updated record,
  `400 Bad Request` if the body sets no field at all, or `404 Not Found` if the key matches
  no row. The empty-body check runs before the pool is touched: an update whose `SET` list is
  built from the fields that are present has no statement to run when none of them are, which
  left to the implementation is a SQL syntax error and a `500`. It also runs before the key is
  looked up, so an empty body is a `400` whether or not the row exists. Bundled into
  `BasicCrudRoutes`, which now covers every CRUD operation
- `crates/axum_helpers/tests/route_responses.rs`: response-code assertions for the route
  handlers, as opposed to the compile-and-mount checks the other test crates do.
  `PgPool::connect_lazy` builds a pool without opening a connection, so a handler can be
  awaited and its status inspected without a database. This is what caught the replace
  handler answering `200` with a `null` body for a missing row
- `axum_helpers::ReplaceRoute` and a matching derive: a `PUT` handler taking the primary key
  from the URL path and a key-less body from JSON, returning `200 OK` with the replaced
  record. Because the body type has no key field, a generated OpenAPI schema omits the key
  from the request body without being told to, while by default a client that sends one
  anyway still succeeds — serde ignores unknown fields. That tolerance is serde's default,
  not a guarantee: a record carrying `#[serde(deny_unknown_fields)]` forwards it to the
  generated body, which then rejects a key-bearing payload with `422`. The attribute is
  deliberately not stripped — an explicit opt-in to strictness is honoured. Bundled into
  `BasicCrudRoutes`, which now covers every CRUD operation
- `sql_traits::Page`, `sql_traits::PaginationParams`, and the offset and cursor parameter and
  metadata types, plus `sql_traits::ListRecordsPaginated<P>` and
  `sql_traits::ListRecordsWherePaginated<T, P>`. Two traits rather than four, because the
  pagination mode is the type parameter `P`: a record type offering both modes implements the
  same trait at `OffsetParams` and at `CursorParams<C>`. `P::Pagination` is the metadata the
  response carries, which is what lets one route handler serve every mode — a total or a next
  cursor can only come from the query that fetched the rows, never from HTTP code afterwards.
  `CursorPagination<C>` is generic over the cursor so `next` goes back out as the type that came
  in, and an implementor whose cursor is a row id allocates nothing in either direction. The doc
  comments carry the four contracts the compiler cannot: a cursor needs a total order, `next`
  comes from a `limit + 1` fetch, `total` is `Some` iff the request asked, and no limit is
  enforced in this crate. `ListRecords` and `ListRecordsWhere<T>` are untouched — a consumer does
  nothing unless it opts in
- `axum_helpers::ListRecordsPaginatedRoute<Q>` and
  `axum_helpers::ListRecordsWherePaginatedRoute<T, Q>`, with the query-string types
  `OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>` and `CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>`
  and the `PaginationQuery` trait pairing each with the validated parameters it resolves to.
  Pagination is a separate route trait rather than a mode of `ListRecordsRoute`, so a response is
  never sometimes an array and sometimes an object: a type opts in by naming the trait, and then
  every response is an envelope, empty pages included. The limit policy is const generic rather
  than a pair of associated consts because that keeps the query-to-params conversion an
  infallible, total `From` — the default limit is stated exactly once, in `Default`, and
  `validate` reads the maximum off the type instead of having it threaded in. A transposed
  `<DEFAULT, MAX>` pair is a compile error. `limit` is consequently a plain `u16` rather than an
  `Option`: the container's `serde` default comes from *this type's* `Default`, so a type whose
  maximum sits below the crate-wide default still serves a request that names no limit, which a
  crate-wide serde default could not. That attribute has to name its path — a bare
  `#[serde(default)]` panics `serde_derive` on a const-generic struct — which makes the struct
  and const parameter names load-bearing, as `crates/axum_helpers/CLAUDE.md` records. The handlers
  extract `Result<Query<Q>, QueryRejection>` so an unparseable query string answers
  `{"message": "..."}` like every other error here instead of axum's plain-text default
- **Breaking.** `axum_helpers::RequestError`, a new `error_set!` subset holding
  `InvalidPaginationLimit { requested, max }`, which is also therefore a new `ApiError` variant.
  `error_set!` generates a plain enum with no `#[non_exhaustive]`, so a consumer that matches
  `ApiError` exhaustively must add an arm or a wildcard. It is a subset rather than inline
  variants so `PaginationQuery::validate` returns only the error it can actually produce and the
  handler widens it with `?`; the status decision stays in the single `From<ApiError> for
  ApiErrorResponse` match where every other status is made

### Changed

- **Breaking.** A composite primary key is now a generated `{Name}PrimaryKey` struct rather
  than a tuple, so `axum::extract::Path` binds each URL segment **by name**. A tuple is
  filled from the segments left to right with no name matching, so a route declaring them in
  a different order from the `#[macros(primary_key)]` fields — say
  `.route("/m/{group_id}/{user_id}", get(Membership::get_record_route))` on a
  `Membership { user_id, group_id }` — compiled, mounted, and ran while addressing the wrong
  row, with no error anywhere; `DeleteRoute` deleted it. Only the routes could see the
  mistake, and only at runtime, and they had no way to report it. Segment order is now
  irrelevant, a segment naming no key field is ignored (so a keyed route can nest under
  unrelated segments), and a key field no segment names is a `400` from the extractor before
  the handler runs. The struct takes the record's own visibility, each field keeps the
  visibility it has on the record, and it always derives `Clone`, `Debug`, `PartialEq` and
  `serde::Deserialize` — `Deserialize` is what every route trait bounds the key on, so it is
  emitted rather than asked for. The record's own attributes are deliberately *not*
  forwarded to it, unlike `{Name}Body` and `{Name}Update`: this type is addressed by path
  segments rather than by JSON, so a `serde` rename meant for a request body has no business
  renaming the segments a route must declare. A **single** marked field is unchanged —
  `PrimaryKey` is still that field's own type, because a lone segment binds unambiguously
  and `Path<i64>` accepts a route whatever it names it. Migrating a composite key means
  replacing tuple access with field access: `let (a, b) = primary_key;` becomes
  `primary_key.a` / `primary_key.b`, and a hand-written `impl GetRecord`/`DeleteRecord`/
  `UpdateRecord` binding the tuple to SQL binds the named fields instead. The name
  `{Name}PrimaryKey` is now reserved alongside `{Name}Body` and `{Name}Update`
- **Breaking.** A composite primary key on a *tuple* struct is a compile error naming the
  shape that works, rather than falling back to the old positional binding. Unnamed fields
  give a path segment nothing to bind to, so it is the one place the bug above could not be
  fixed. A single marked field on a tuple struct is unaffected
- **Breaking.** The SQL traits whose names did not match their methods are renamed:
  `InsertSQL::insert_sql` to `InsertRecord::insert_record`, `BulkInsertSQL::bulk_insert_sql`
  to `BulkInsertRecords::bulk_insert_records`, `DeleteSQL::delete_sql` to
  `DeleteRecord::delete_record`, and the two list methods `ListRecords::get_all` and
  `ListRecordsWhere::get_records` to `list_records` and `list_records_where`. Every trait in
  `sql_traits` now pairs a `<Verb><Noun>` name with a method spelling the same words, which
  `GetRecord`, `GetRecordWhere`, `GetLatestRecord`, `DeleteRecordsWhere`, `ReplaceRecord`, and
  `UpdateRecord` already did — the `*SQL` suffix and the `get_*` list methods were the only
  holdouts. Singular and plural now carry meaning as well: `InsertRecord` and `DeleteRecord`
  address one row, `BulkInsertRecords` and `DeleteRecordsWhere` address many. Both halves of
  the rename fail loudly at the use site — a stale `impl InsertSQL for T` gets "cannot find
  trait", a stale `t.insert_sql(&pool)` gets "no method named `insert_sql`" — and no retired
  name is reused, so nothing can silently rebind to a different trait
- **Breaking.** `#[derive(BasicCrudRoutes)]` now emits `impl ReplaceRoute` and
  `impl UpdateRoute` alongside the five route impls it already emitted, so the bundle covers
  every CRUD operation rather than everything except update: create (`CreateRoute`,
  `BulkCreateRoute`), read (`GetRecordRoute`, `ListRecordsRoute`), update (`ReplaceRoute`,
  `UpdateRoute`), delete (`DeleteRoute`). Types deriving it today need `HasRequestBody`,
  `ReplaceRecord`, `HasUpdateFields`, and `UpdateRecord` impls added — in practice by
  deriving `Record` and `Update` alongside it, which supply the three association impls and
  generate the body and update types the write routes take as request bodies. As with any
  unmet supertrait, each missing one is an error on the generated impl naming the trait it
  cannot find, not on the derive. `GetLatestRoute` stays excluded: "the most recent row" is a
  domain-specific query rather than a CRUD operation, and bundling it would force every
  deriving type to implement `GetLatestRecord`
- **Breaking.** `sql_traits::ReplaceRecord::replace_record` returns `Result<Option<Self>, _>`
  rather than `Result<Self, _>`, and `axum_helpers::ReplaceRoute` answers `404 Not Found`
  when it gets `None` — the convention `GetRecordRoute` already followed. Previously a `PUT`
  to a key matching no row forced the implementation to report `sqlx::Error::RowNotFound`,
  which the error mapping turned into a `500`. Existing impls need `Ok(record)` changed to
  `Ok(Some(record))`; the compiler names every one of them
- **Breaking.** `sql_traits::UpdateRecord` takes `HasUpdateFields` as a supertrait instead of
  declaring its own `type UpdateFields`, so the record/fields pair is the single source of
  truth for what a partial update is and an implementation cannot pair a record with a fields
  type that does not point back at it. Its return type gains the same `Option` as
  `ReplaceRecord`, for the same reason. The trait was added earlier on this branch and has not
  been released, so nothing downstream depends on the old shape
- `#[macros(...)]` container directives are now routed by which derive reads them, rather than
  each derive rejecting everything it does not consume. `Record` reads `body_derive` and steps
  over `update_derive`; `Update` does the reverse; `PrimaryKey` reads neither but tolerates
  `update_derive`, since `#[derive(PrimaryKey, Update)]` is a valid pairing. It still rejects
  `body_derive`, which nothing on such a struct can ever read. A derive macro cannot see its
  siblings, so this is the only way one struct can carry both lists. An unrecognized directive
  is still a `compile_error!`, and a struct-level directive written on a field now names the
  directive that was actually misplaced instead of listing every one it could have been
- **Breaking.** The route derives emit impls bounded on the renamed traits, so a type deriving
  `BasicCrudRoutes`, `CreateRoute`, `BulkCreateRoute`, or `DeleteRoute` needs its `InsertSQL`,
  `BulkInsertSQL`, and `DeleteSQL` impls renamed to match. As with any unmet supertrait, the
  error lands on the generated impl naming the trait it cannot find, not on the derive

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

### Removed

- **Breaking.** `#[derive(BasicCrudRoutes)]` no longer emits `impl GetLatestRoute`. "The most recent row" is a domain-specific query rather than a CRUD operation, and bundling it forced every type deriving `BasicCrudRoutes` to implement `GetLatestRecord` whether or not it had a meaningful notion of "latest". The `GetLatestRoute` trait and its standalone derive are unchanged: types that want the handler add `#[derive(GetLatestRoute)]` alongside `BasicCrudRoutes`

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
