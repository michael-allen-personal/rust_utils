# Changelog

## v1.0.0

### Upgrading from v0.7.0

Each entry below says what it requires on its own; this is the same list in one place, in the
order that keeps the compiler's errors readable.

1. **Manifest.** Delete `macros` from `Cargo.toml`. Declare `sql_traits` alongside
   `axum_helpers`, pinned to the same tag, and enable a driver feature on `axum_helpers`:
   `features = ["postgres"]` (or `sqlite`, `mysql`, `any`). It forwards to `sql_traits`, so
   that line needs one only if `sql_traits` is the only one of the two you depend on.
2. **Derive paths.** `macros::{PrimaryKey, Record, Update, Database}` → `sql_traits::…`,
   `macros::{*Route, BasicCrudRoutes}` → `axum_helpers::…`, `macros::MaxVecCapacity` →
   `generic_helpers::MaxVecCapacity`, and `#[macros(` → `#[sql_traits(`.
3. **`HasDatabase`.** Every type implementing a pool-taking SQL trait or a route trait adds
   `#[derive(sql_traits::Database)]` with `#[sql_traits(database = Postgres)]`.
4. **Renames.** `InsertSQL` → `InsertRecord`, `BulkInsertSQL` → `BulkInsertRecords`,
   `DeleteSQL` → `DeleteRecord`, `ListRecords::get_all` → `list_records`,
   `ListRecordsWhere::get_records` → `list_records_filtered`, then `Where` → `Filtered` and
   `_where` → `_filtered` everywhere.
5. **Filtered routes.** `type PathParams` → `type FilterParams`, as a struct with named fields.
6. **Composite keys.** Tuple access on a composite primary key becomes field access.
7. **`BasicCrudRoutes`.** Types deriving it also derive `sql_traits::Record` and
   `sql_traits::Update` and implement `ReplaceRecord` and `UpdateRecord`.
8. **`DeleteRoute`.** `DeleteRecord::ReturnType` must be `Serialize`.
9. **`ApiError`.** An exhaustive `match` gains four arms: `InvalidPaginationLimit`,
   `InvalidQueryParams`, `InvalidFilterParams`, `InvalidPathParams`.
10. **Behaviour with no compile error.** Check create and delete response statuses, query keys
    now filling filter fields, `#[serde(deny_unknown_fields)]` on filters, and `ApiError::Serde`
    answering `500` — each described in its entry under Changed.

### Added

- `generic_helpers` crate, moved here from the `data-monorepo` repo (where it was
  `common_parser`). Holds the `str_enum!` macro, `ParsingError`, and the
  `buf_reader_from_path` / `MaxVecCapacity` file helpers. Its only external dependency is
  `error_set`
- `generic_helpers::MaxVecCapacity` derive macro, re-added now that the trait lives in this
  workspace. It emits `impl ::generic_helpers::MaxVecCapacity for T {}` — an absolute path
  through `generic_helpers`' root re-export, unlike the removed version's bare
  `common_parser::` path
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
- `#[derive(sql_traits::Record)]`: a superset of `PrimaryKey` that also generates `{Name}Body`
  from the fields _not_ marked `#[sql_traits(primary_key)]`, both association impls, and both
  `From` conversions — `From<(PrimaryKey, Body)> for Record` and `From<Record> for Body`.
  Together those two make `record -> (key, body) -> record` expressible as a single
  round-trip assertion, which is the test that catches a field reassembled into the wrong
  slot. Name the body type's derives with `#[sql_traits(body_derive(...))]`: a derive macro
  cannot see sibling `#[derive(...)]` attributes, since rustc strips them before it runs.
  Every other attribute forwards to the body automatically, which is what keeps `serde` and
  `ts-rs` renames from drifting between a record and its body. Named-field structs only.
  A `#[sql_traits(...)]` attribute that is not a recognised directive — a typo, a stray
  comma, `body_derive` in a non-list form, or a directive on the wrong item — is a
  `compile_error!` naming what was found and what is accepted, rather than being ignored.
  Ignoring it would be dangerous: a malformed `#[sql_traits(primary_key,)]` would drop the
  field out of the key and _into_ the generated body, leaking the key into the request body
- `sql_traits::HasUpdateFields` and `sql_traits::UpdateFields`: a pair of traits associating
  a record with the type holding a _partial_ set of its non-key fields, mirroring
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
- `#[derive(sql_traits::Update)]`: generates `{Name}Update` from the fields _not_ marked
  `#[sql_traits(primary_key)]`, each wrapped in one more `Option` than the record has —
  `String` becomes `Option<String>`, `Option<i32>` becomes `Option<Option<i32>>` — plus both
  association impls. Name its derives with `#[sql_traits(update_derive(...))]`, the
  counterpart to `body_derive`. A field whose type is syntactically `Option<..>` also gets
  `#[serde(default, deserialize_with = "::sql_traits::double_option")]`, which is what makes
  an explicit `null` clear the column. A type _alias_ for `Option<T>` cannot be recognized —
  no proc macro can resolve one — and such a field simply loses the ability to be cleared;
  it is never a type error, because the generated field type is correct either way.
  Designed to sit alongside `#[derive(sql_traits::Record)]`, which is what supplies the
  `HasPrimaryKey` impl that `HasUpdateFields` requires. Each derive reads only its own
  struct-level directive and steps over the other's, since a derive macro cannot see its
  siblings and this is the only way one struct can carry both lists: `Record` reads
  `body_derive`, `Update` reads `update_derive`, and `PrimaryKey` reads neither but tolerates
  `update_derive`, since `#[derive(PrimaryKey, Update)]` is a valid pairing. It still rejects
  `body_derive`, which nothing on such a struct can ever read. A struct-level directive
  written on a field is an error naming that directive
- `sql_traits::ReplaceRecord` and `sql_traits::UpdateRecord`: the SQL side of a full and a
  partial update by primary key. Both return `Result<Option<Self>, _>`, where `None` means no
  row matched — the convention `GetRecord` already follows, and what lets the route traits
  answer `404` instead of forcing an implementation to report `sqlx::Error::RowNotFound`,
  which maps to a `500`. `UpdateRecord` takes `HasUpdateFields` as a supertrait rather than
  declaring its own fields type, so the record/fields pair is the single source of truth for
  what a partial update is and an implementation cannot pair a record with a fields type that
  does not point back at it
- `axum_helpers::ReplaceRoute` and a matching derive: a `PUT` handler taking the primary key
  from the URL path and a key-less body from JSON, returning `200 OK` with the replaced
  record, or `404 Not Found` if the key matches no row. Because the body type has no key
  field, a generated OpenAPI schema omits the key from the request body without being told
  to, while by default a client that sends one anyway still succeeds — serde ignores unknown
  fields. That tolerance is serde's default, not a guarantee: a record carrying
  `#[serde(deny_unknown_fields)]` forwards it to the generated body, which then rejects a
  key-bearing payload with `422`. The attribute is deliberately not stripped — an explicit
  opt-in to strictness is honoured
- `axum_helpers::UpdateRoute` and a matching derive: a `PATCH` handler taking the primary key
  from the URL path and a partial body from JSON, returning `200 OK` with the updated record,
  `400 Bad Request` if the body sets no field at all, or `404 Not Found` if the key matches
  no row. The empty-body check runs before the pool is touched: an update whose `SET` list is
  built from the fields that are present has no statement to run when none of them are, which
  left to the implementation is a SQL syntax error and a `500`. It also runs before the key is
  looked up, so an empty body is a `400` whether or not the row exists
- `sql_traits::Page`, `sql_traits::PaginationParams`, and the offset and cursor parameter and
  metadata types, plus `sql_traits::ListRecordsPaginated<P>` and
  `sql_traits::ListRecordsFilteredPaginated<T, P>`. Two traits rather than four, because the
  pagination mode is the type parameter `P`: a record type offering both modes implements the
  same trait at `OffsetParams` and at `CursorParams<C>`. `P::Pagination` is the metadata the
  response carries, which is what lets one route handler serve every mode — a total or a next
  cursor can only come from the query that fetched the rows, never from HTTP code afterwards.
  `CursorPagination<C>` is generic over the cursor so `next` goes back out as the type that came
  in, and an implementor whose cursor is a row id allocates nothing in either direction. `Page`,
  `OffsetPagination` and `CursorPagination<C>` derive `Deserialize`, `PartialEq` and `Eq` as
  well as `Serialize`, so a Rust client or an integration test can parse the envelope back into
  the same types the handler sent and compare it with `assert_eq!`. Those derives generate
  bounded impls and put no requirement on the record or metadata type: a `Serialize`-only record
  still builds and serves a `Page`. The doc comments carry the four contracts the compiler
  cannot: a cursor needs a total order, `next` comes from a `limit + 1` fetch, `total` is `Some`
  iff the request asked, and no limit is enforced in this crate. For an impl whose cursor is
  opaque, decoding belongs in the cursor type's own `Deserialize`, where a malformed cursor
  becomes a query-string rejection and a `400` instead of the `500` every `sqlx::Error` maps to.
  `ListRecords` and `ListRecordsFiltered<T>` are untouched — a consumer does nothing unless it
  opts in
- `axum_helpers::ListRecordsPaginatedRoute<Q>` and
  `axum_helpers::ListRecordsFilteredPaginatedRoute<T, Q>`, with the query-string types
  `OffsetParamsQuery<DEFAULT_LIMIT, MAX_LIMIT>` and `CursorParamsQuery<C, DEFAULT_LIMIT, MAX_LIMIT>`
  and the `PaginationQuery` trait pairing each with the validated parameters it resolves to.
  Pagination is a separate route trait rather than a mode of `ListRecordsRoute`, so a response is
  never sometimes an array and sometimes an object: a type opts in by naming the trait, and then
  every response is an envelope, empty pages included. **Neither const parameter has a
  default**, so both numbers are always written together; the crate's own policy (50 per page,
  200 at most) arrives under a name, `DefaultOffsetParamsQuery` and `DefaultCursorParamsQuery<C>`.
  A default would make `OffsetParamsQuery<10>` legal, which reads as "cap this route at 10" but
  would mean a default of 10 under the inherited maximum of 200. A transposed or zero-defaulted
  `<DEFAULT, MAX>` pair fails as a post-monomorphization error: `cargo build` and `cargo test`
  report it, and `cargo check` — or an editor running it — does not. Because each type carries
  its own default, a route whose maximum sits below the crate-wide default still serves a request
  that names no limit. The handlers extract `Result<Query<Q>, QueryRejection>` so an unparseable
  query string answers `{"message": "..."}` like every other error here instead of axum's
  plain-text default. The policy bounds page _size_ only: there is deliberately no ceiling on
  `offset`, and a consumer who needs to bound how deep a request may page does it in their own
  query
- **Breaking.** `axum_helpers::ValidationError`, a new `error_set!` subset holding
  `InvalidPaginationLimit { requested, max }`, which is also therefore a new `ApiError` variant.
  `error_set!` generates a plain enum with no `#[non_exhaustive]`, so a consumer that matches
  `ApiError` exhaustively must add an arm or a wildcard. It is a subset rather than inline
  variants so `PaginationQuery::validate` returns only the error it can actually produce, and
  the handler converts it with `ApiError::from`; the status decision stays in the single
  `From<ApiError> for ApiErrorResponse` match where every other status is made
- **Breaking.** `ApiError::InvalidQueryParams(axum::extract::rejection::QueryRejection)`, the
  variant a query string axum could not deserialize arrives as — an unparseable `?limit=abc`, or
  a cursor type whose own `Deserialize` rejected what it was handed. Another exhaustive `match`
  on `ApiError` to update, for the same reason as above. It is inline on `ApiError` rather than
  in `ValidationError` despite being the client's fault, and both reasons come from the source
  type: `QueryRejection` is neither `PartialEq` nor `Eq`, so holding it would cost `ValidationError`
  the derive its `assert_eq!` assertions depend on, and no validation function can _produce_ one
  — it arrives already formed from the extractor — so putting it in the subset would widen
  `PaginationQuery::validate`'s return type to something it can never return. The client gets
  exactly the words axum would have sent as plain text, wrapped in this crate's
  `{"message": "..."}` object
- `IntoResponse` and `Into<ApiErrorResponse>` for `axum_helpers::IOError` and
  `axum_helpers::ValidationError`, so a consumer's handler can return the narrow error it actually
  produces and have it render itself, rather than widening to `ApiError` by hand at every call
  site. Both route through `From<ApiError>`, so a subset and the whole cannot answer different
  statuses for the same variant
- `sql_traits::HasDatabase`, associating a record with the one database its queries run
  against. The database travels as an associated type rather than a parameter on each
  trait, which is what keeps every trait's parameter list and every axum mount site
  unchanged: a record names its database once and the thirteen pool-taking traits follow
  it. It is deliberately not a supertrait of `HasPrimaryKey` — the key, request-body and
  update-fields machinery never touches a pool and stays database-agnostic
- `#[derive(sql_traits::Database)]`, reading `#[sql_traits(database = Sqlite)]` and emitting
  the `HasDatabase` impl. Standalone rather than folded into `Record`, so a hand-written record
  and a separate insert-side type can both use it. The directive is required; its absence
  is a compile error naming the accepted set (`Postgres`, `Sqlite`, `MySql`, `Any`) rather
  than a silent default
- Driver features on `sql_traits` and `axum_helpers` (`postgres`, `sqlite`, `mysql`,
  `any`), forwarding to `sqlx`. They exist so a consumer can pick a driver while still
  depending on the re-exported `sqlx` rather than declaring it themselves
- **Sample API (`examples/sample_api`).** A runnable axum application over an in-memory
  SQLite database that implements every pool-taking `sql_traits` trait and every
  `axum_helpers` route trait (thirteen of each): `Author` for the `BasicCrudRoutes` bundle,
  `Book` for a-la-carte route derives, both pagination modes and the `*Filtered` family, and
  `Review` for a composite primary key. It exists so a change to these crates can be
  exercised over real HTTP — `cargo run -p sample_api`, then the requests in
  `examples/sample_api/requests.http` — instead of publishing a version and updating a
  downstream project.

  It also acts as a fifth consumer-perspective compile check. Like the `tests/` crates, it
  reaches `axum`, `serde`, `sqlx` and `sql_traits` only through `axum_helpers`' re-exports,
  so a re-export that stops being sufficient to write a real application breaks the
  workspace build rather than surfacing in a consumer's repo.

  Additive: no existing crate changed, and consumers need do nothing.

- `axum_helpers::PathAndQuery<T>`: an axum extractor filling one `T` from the route's path
  segments and the query string together, the extractor every `*FilteredRoute` now reads its
  filter through. It is public so a hand-written handler can read a filter exactly the way the
  route traits do, with the same duplicate-key `400` and the same error shape — it rejects with
  `ApiError`. The path segments are re-encoded and parsed alongside the raw query by
  `serde_urlencoded`, the parser behind axum's own `Query`, so a field parses identically under
  either extractor. `serde_urlencoded` is re-exported as `axum_helpers::serde_urlencoded`, since
  its error type is the source of `ApiError::InvalidFilterParams`.

### Changed

- **Breaking.** The `macros` crate is gone. Its fourteen derives now ship from the crates
  whose traits they implement: `sql_traits::{PrimaryKey, Record, Update, Database}`,
  `axum_helpers::{CreateRoute, BulkCreateRoute, GetRecordRoute, GetLatestRoute,
  ListRecordsRoute, ReplaceRoute, UpdateRoute, DeleteRoute, BasicCrudRoutes}`, and
  `generic_helpers::MaxVecCapacity` — the arrangement `serde` and `sqlx` use, where a crate
  hands out its own derives and the proc-macro crate behind them is private plumbing
  (`sql_traits_macros`, `axum_helpers_macros`, `generic_helpers_macros`, none of which a
  consumer names). No generated path changed; the derives only moved.

  The reason is that a standalone public macro crate forced consumers to declare a crate
  their source never named. Generated code anchors at absolute paths — `::sql_traits::HasPrimaryKey`
  — and an absolute path resolves only where the *use site* declares that crate, so deriving
  `macros::Record` required a `sql_traits` line in `Cargo.toml` that nothing in the consumer's
  own source explained, with a missing one erroring at the derive rather than at the manifest.
  Writing `sql_traits::Record` makes the manifest entry self-evident: the spelling cannot be
  written without the dependency it needs. Each macro crate now also carries an
  `every_emitted_path_anchors_at_*` test, so the rule that used to live in prose is checked
  by the build.

  The helper attribute is renamed to match its new home in the same release, rather than
  breaking twice: `#[macros(primary_key)]` becomes `#[sql_traits(primary_key)]`, and likewise
  for `body_derive`, `update_derive` and `database`. The directive names are unchanged.

  To migrate: delete `macros` from `Cargo.toml`; make sure `sql_traits` is declared (most
  consumers already declare it, for the extern-prelude reason this change removes); rewrite
  `macros::{PrimaryKey,Record,Update,Database}` to `sql_traits::…`, `macros::{*Route,BasicCrudRoutes}`
  to `axum_helpers::…`, and `macros::MaxVecCapacity` to `generic_helpers::MaxVecCapacity`;
  then `#[macros(` to `#[sql_traits(`. No driver feature is needed on the `sql_traits` line —
  cargo unifies the one already set on `axum_helpers` onto the same compiled copy

- **Breaking.** The `*Where` traits are renamed `*Filtered` in both crates, along with their
  methods: `GetRecordWhere` → `GetRecordFiltered`, `ListRecordsWhere` → `ListRecordsFiltered`,
  `DeleteRecordsWhere` → `DeleteRecordsFiltered`, each route trait to the matching
  `*FilteredRoute` (`GetRecordWhereRoute` → `GetRecordFilteredRoute`, and so on), and every
  `*_where` / `*_where_route` method to `*_filtered` / `*_filtered_route`
  (`get_record_where` → `get_record_filtered`, `delete_records_where_route` →
  `delete_records_filtered_route`). The pagination traits and the `delete_records_filtered_response`
  hook added in this release are spelled the new way from the start, and the methods' `where_params`
  parameter is now `filter_params`. "Where" named the SQL clause an implementation happens to
  write, while a consumer thinks of these as filtered fetches. That mattered more once the
  filter could come from the query string (issue #23, below) and became the `FilterParams`
  associated type. Each route trait still pairs one-to-one with its SQL trait. No retired name is
  reused, so a stale `impl GetRecordWhere<F>` gets "cannot find trait" and a stale mount gets
  "no function or associated item named `get_record_where_route`". To migrate, rename
  `Where` → `Filtered` and `_where` → `_filtered` in trait names, method names and mount
  sites. A find-and-replace on exactly those two substrings is safe, because neither one
  occurs in Rust's `where` keyword.

- **Breaking.** `CreateRoute`, `BulkCreateRoute`, `DeleteRoute` and `DeleteRecordsFilteredRoute`
  now decide whether to send a body from what their SQL trait's `ReturnType` serializes to.
  A value serializing to JSON `null` — `()`, a unit struct, `None` — answers `204 No Content`
  for a delete and a bodyless `201 Created` for a create; anything else answers `200`/`201`
  with the value as JSON. Each route renders that through a new provided hook,
  `create_response`, `bulk_create_response`, `delete_response` and
  `delete_records_filtered_response`, which an impl overrides to answer differently. The hook is
  only ever called with a success, so an override cannot change the `{"message": …}` error
  shape. (#11)

  The old responses were hardcoded, and two of them were wrong. `DeleteRoute` always answered
  `204` and threw away whatever `DeleteRecord` returned, so a delete that counted rows could
  not report the count. The others always serialized, so a `()` return — which
  `BulkInsertRecords`' own documentation suggests for bulk inserts — answered `201` or `200`
  with a body of `null`. A trait for return types to implement was considered and rejected:
  every consumer summary type would have needed an impl or a wrapper. Reading the serialized
  bytes needs neither, and the value is serialized for the response anyway.

  Only one part of this is a compile error. `DeleteRoute` now requires
  `DeleteRecord::ReturnType: Serialize`, as the other three already required of theirs; derive
  `Serialize` on the type or return `()`. A generic function bounded on `DeleteRoute` has to
  restate that bound, the same way it already restates `PrimaryKey: DeserializeOwned`. **The
  rest changes behaviour with nothing to warn you:**

  - A `DeleteRecord` returning a value, such as a `u64` row count, answered `204` and now
    answers `200` with the value. To keep `204`, return `()` or override `delete_response`.
  - A `DeleteRecordsFiltered` returning `()` answered `200` with `null` and now answers `204`.
  - An `InsertRecord` or `BulkInsertRecords` returning `()` answered `201` with `null` and
    now answers `201` with no body and no `Content-Type`.
  - A `ReturnType` of `Option<T>` returning `None` now gets the no-body status. It is never
    read as "not found"; a delete that should answer `404` for a missing key overrides
    `delete_response`, as `sample_api`'s `Book` does.

  Clients that parsed the old `null` body, or that treated a delete's `204` as the only
  success, need checking along with the server.

- **Breaking.** The `*Filtered` route traits — `GetRecordFilteredRoute`, `ListRecordsFilteredRoute`,
  `ListRecordsFilteredPaginatedRoute` and `DeleteRecordsFilteredRoute` — now read their filter from
  the URL path *and* the query string, and their associated type is renamed from `PathParams` to
  `FilterParams`, since it no longer comes only from the path and "filter" is what it is to a
  caller. The path segments and the query string are merged into one set of
  keys and deserialized once, through the new `PathAndQuery` extractor, so one filter type can
  take `book_id` from `/books/{book_id}/reviews` and `min_rating` from `?min_rating=4`, and a
  route with no segments at all (`/books?genre=Fiction`) can be filtered purely from the query.
  Which source fills a field is decided by the route a handler is mounted on, not by the type.

  The reason is issue #23: filtering a `GET` from the query string is the common case, and the
  family could only filter on what the path carried. No `sql_traits` change was needed — the
  SQL traits already took an arbitrary filter `T` without knowing where it came from — so the
  whole change is in where the route handler fills `T` from. Separate `PathParams` and
  `QueryParams` types were considered and rejected: every impl would have named both, usually
  one as `()`, and nearly every filter would have needed a hand-written
  `From<(PathParams, QueryParams)>`. Merged, the common case stays
  `type FilterParams = Filter;` with no conversion written.

  A key supplied by both the path and the query is a `400` ("duplicate field"), never an
  override, so a query cannot redirect a request away from the record its path addresses. A
  filter that cannot be filled is a `400` in this crate's `{"message": …}` shape, through the
  new `ApiError::InvalidFilterParams` (or `ApiError::InvalidPathParams`, for a path segment that
  is not valid UTF-8).

  To migrate:

  - Rename `type PathParams = …;` to `type FilterParams = …;` in every `*FilteredRoute` impl.
  - A `FilterParams` that was a bare scalar or a tuple (`type PathParams = i64;`) must become a
    struct with named fields: there are no names to merge a positional value on. Its fields are
    named after the route's segments, as a composite primary key's already are.
  - Code that calls a `*Filtered` handler directly, rather than mounting it, passes
    `PathAndQuery(filter)` where it passed `Path(filter)`.
  - Behaviour changes silently in two places. A query key matching a filter field used to be
    ignored and now fills it — or is a `400` if the path already bound it. And a filter marked
    `#[serde(deny_unknown_fields)]` now sees the whole query string, so on a paginated route it
    rejects `limit`, `offset` and `cursor`; drop the attribute from a filter that shares a query
    string with anything else. A `match` over `ApiError` gains two variants.

- **Breaking.** `ApiError::Serde` now renders a `500` rather than a `400`. It reads like the
  client's fault, which is why it was grouped with the bad requests, but no request this crate
  serves can reach that variant: a JSON body is deserialized by axum's `Json` extractor, which
  rejects with its own `400` before the handler is called, and a query string arrives as
  `ApiError::InvalidQueryParams`. What is left is the server failing to serialize a value of
  its own, which the client can do nothing about and cannot fix by retrying differently. The
  status decision now follows `error_set` membership exactly: every `IOError` variant is a
  `500`, and a variant that can be the client's fault belongs in `ValidationError` or inline on
  `ApiError` instead. A consumer asserting `400` on a serde failure, or funnelling its own
  deserialization errors through `ApiError::Serde` to get one, has to change: raise the
  client-caused case as its own variant rather than reusing this one
- **Breaking.** A composite primary key is now a generated `{Name}PrimaryKey` struct rather
  than a tuple, so `axum::extract::Path` binds each URL segment **by name**. A tuple is
  filled from the segments left to right with no name matching, so a route declaring them in
  a different order from the `#[sql_traits(primary_key)]` fields — say
  `.route("/m/{group_id}/{user_id}", get(Membership::get_record_route))` on a
  `Membership { user_id, group_id }` — compiled, mounted, and ran while addressing the wrong
  row, with no error anywhere; `DeleteRoute` deleted it. Only the routes could see the
  mistake, and only at runtime, and they had no way to report it. Segment order is now
  irrelevant, a segment naming no key field is ignored (so a keyed route can nest under
  unrelated segments), and a key field no segment names is a `400` from the extractor before
  the handler runs. The struct takes the record's own visibility, each field keeps the
  visibility it has on the record, and it always derives `Clone`, `Debug`, `PartialEq` and
  `serde::Deserialize` — `Deserialize` is what every route trait bounds the key on, so it is
  emitted rather than asked for. The record's own attributes are deliberately _not_
  forwarded to it, unlike `{Name}Body` and `{Name}Update`: this type is addressed by path
  segments rather than by JSON, so a `serde` rename meant for a request body has no business
  renaming the segments a route must declare. A **single** marked field is unchanged —
  `PrimaryKey` is still that field's own type, because a lone segment binds unambiguously
  and `Path<i64>` accepts a route whatever it names it. Migrating a composite key means
  replacing tuple access with field access: `let (a, b) = primary_key;` becomes
  `primary_key.a` / `primary_key.b`, and a hand-written `impl GetRecord`/`DeleteRecord`
  binding the tuple to SQL binds the named fields instead. The name `{Name}PrimaryKey` is
  reserved, alongside `{Name}Body` and `{Name}Update`
- **Breaking.** A composite primary key on a _tuple_ struct is a compile error naming the
  shape that works, rather than falling back to the old positional binding. Unnamed fields
  give a path segment nothing to bind to, so it is the one place the bug above could not be
  fixed. A single marked field on a tuple struct is unaffected
- **Breaking.** The SQL traits whose names did not match their methods are renamed:
  `InsertSQL::insert_sql` to `InsertRecord::insert_record`, `BulkInsertSQL::bulk_insert_sql`
  to `BulkInsertRecords::bulk_insert_records`, `DeleteSQL::delete_sql` to
  `DeleteRecord::delete_record`, and the two list methods `ListRecords::get_all` and
  `ListRecordsWhere::get_records` to `list_records` and `list_records_filtered` (the trait itself
  is now `ListRecordsFiltered`; see the `*Where` → `*Filtered` rename above). Every trait in
  `sql_traits` now pairs a `<Verb><Noun>` name with a method spelling the same words, as
  `GetRecord` and `GetLatestRecord` already did — the `*SQL` suffix and the `get_*` list
  methods were the only holdouts. Singular and plural now carry meaning as well: `InsertRecord`
  and `DeleteRecord` address one row, `BulkInsertRecords` and `DeleteRecordsFiltered` address
  many. Both halves of the rename fail loudly at the use site — a stale `impl InsertSQL for T`
  gets "cannot find trait", a stale `t.insert_sql(&pool)` gets "no method named `insert_sql`" —
  and no retired name is reused, so nothing can silently rebind to a different trait. The route
  derives emit impls bounded on the new names, so a type deriving `BasicCrudRoutes`,
  `CreateRoute`, `BulkCreateRoute` or `DeleteRoute` needs its SQL impls renamed to match; as
  with any unmet supertrait, the error lands on the generated impl naming the trait it cannot
  find, not on the derive
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
- **Breaking.** All thirteen pool-taking SQL traits and all thirteen route traits now take
  `Pool<<Self as HasDatabase>::Database>` in place of `PgPool`, and every type implementing
  one must implement `HasDatabase`. Add `#[derive(sql_traits::Database)]` with
  `#[sql_traits(database = Postgres)]` to each record, or write the three-line impl by hand.
  Existing method bodies and signatures do not change: `Pool<Postgres>` *is* `PgPool`, so an
  impl written `async fn get_record(pool: &PgPool, ...)` still satisfies the new signature
  once its `Database` is `Postgres`
- **Breaking.** Neither crate enables a `sqlx` driver any more, so a consumer must enable
  one: `axum_helpers = { ..., features = ["postgres"] }`, which `axum_helpers` forwards to
  `sql_traits`, or the same on `sql_traits` for a consumer that depends on it alone.
  Without it, nothing names a driver and `sqlx::Postgres` will not resolve. Note that cargo
  feature unification can mask this — if any other crate in your graph enables
  `sqlx/postgres`, the build may succeed until that dependency changes. Set the feature
  explicitly rather than relying on it. The upside is that a SQLite-only consumer no longer
  compiles `sqlx-postgres` at all

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
