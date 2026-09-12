# sample_api

A runnable axum application over an in-memory SQLite database, implementing every
pool-taking `sql_traits` trait and every `axum_helpers` route trait — thirteen of each —
once: `Author` for the `BasicCrudRoutes` bundle, `Book` for a-la-carte route derives plus
both pagination modes and the `*Where` family, and `Review` for a composite primary key. It
is not a library and ships nothing anyone depends on.

**It is two things at once: a manual test surface, and a compile check.** `cargo run -p
sample_api`, then the requests in `requests.http`, is how a change to these crates gets
exercised over real HTTP without publishing a version and updating some other repo to
pull it in. The second job is quieter but is why the crate exists as a workspace member
rather than a directory `cargo` ignores: a workspace member is built by `cargo test` and
`cargo build --workspace`, so a re-export that stops being enough to write a real
application fails *here*, in this repo, rather than in a downstream consumer's build. An
excluded example directory would still run by hand but would never be built by CI, which
defeats the point.

## Every leaked type comes from `axum_helpers`, and `sqlx` is in the manifest for a feature flag, not for its API

`Cargo.toml` depends on `axum_helpers`, `generic_helpers`, `macros`, `sql_traits` and
`sqlx` — but no source file writes `use sqlx::...` or names an `axum` or `serde` type that
didn't come through `axum_helpers::{axum, serde, sqlx, sql_traits, async_trait}`. `sqlx` is
declared for one reason: `features = ["runtime-tokio"]` selects the async runtime the
binary needs for `#[tokio::main]` and `TcpListener`, a choice `axum_helpers` has no opinion
on and cannot make on this crate's behalf. The driver itself (`sqlite`) is still selected
through `axum_helpers`' forwarded feature, not through this `sqlx` dependency. If this
crate ever needed to name a `sqlx` type directly to get something built, that would be a
finding about the library's re-export surface, not a reason to add the import.

## The `sql_traits` finding

This is the most useful thing this sample turned up, and it is a fact about the macros,
not about the sample.

`macros::Record`, `macros::Update`, `macros::Database` and `macros::PrimaryKey` emit *bare*
`::sql_traits::…` paths — `::sql_traits::HasPrimaryKey` at
`crates/macros/src/lib.rs:311`, `::sql_traits::HasRequestBody` at line 579,
`::sql_traits::HasUpdateFields` at line 722, `::sql_traits::HasDatabase` at line 749, among
others. A bare `::sql_traits::` resolves only where `sql_traits` is a name in the deriving
crate's own extern prelude — that is, only where the crate that writes `#[derive(macros::Record)]`
also lists `sql_traits` in its own `[dependencies]`. Going through `axum_helpers`'
re-export is not an option for these four: they have nothing to do with axum and are used
by non-HTTP consumers of `sql_traits` alone, so the path they emit cannot be conditional on
`axum_helpers` being present. The one generator that *can* assume axum is in the room,
`insert_route_impl` (`crates/macros/src/lib.rs:188`, behind `macros::CreateRoute` and
`macros::BulkCreateRoute`), does exactly that — it emits `::axum_helpers::sql_traits::#sql_trait`
instead, because an insert route is axum-specific by construction.

Emitting `::axum_helpers::sql_traits::…` from the other four generators as well was
considered and rejected. It would let a deriving crate skip declaring `sql_traits`
directly, but only by making every non-HTTP consumer of `Record`/`Update`/`Database`/
`PrimaryKey` — the ones the root `CLAUDE.md` says must be able to use the database traits
without axum — depend on `axum_helpers` just to resolve a path. That is precisely the
layering the workspace keeps one-way, so the fix is what this crate's `Cargo.toml` actually
does: declare `sql_traits` as a direct dependency, purely for the extern prelude, with no
source file importing anything from it.

This had never been visible from *inside* the workspace before this crate existed. Every
prior consumer of these derives lives under `sql_traits/tests/` or `axum_helpers/tests/`,
and a package's own `[dependencies]` are automatically in scope for its `tests/` targets —
so `sql_traits/tests/derive_macros.rs` gets `sql_traits` in its prelude for free, by
accident of where it lives, never by needing to state it. `sample_api` is the first use
site that derives `Record`/`Update`/`Database` without that accident, and it did not build
— `E0433: failed to resolve: use of undeclared crate or module 'sql_traits'` — until
`sql_traits = { workspace = true }` was added to its manifest.

**The practical consequence for any downstream repo:** using `macros::Record`,
`macros::Update`, `macros::Database` or `macros::PrimaryKey` requires declaring `sql_traits`
directly, alongside `axum_helpers`, even if nothing in the crate ever writes `sql_traits::`
by hand. Reaching only `axum_helpers` and expecting the derives to work is the exact
mistake this sample caught.

## What each module demonstrates

- `db.rs` — the schema, the seed data, and the single `connect_and_seed` every route shares
  through axum's `State`.
- `authors.rs` — the batteries-included path. One `#[derive(macros::BasicCrudRoutes)]`
  implements seven route traits at once; `GetLatestRoute` is derived separately because the
  bundle deliberately excludes it.
- `books.rs` — the a-la-carte path, plus pagination and the `*Where` family.
- `reviews.rs` — the composite-key path: two fields marked `#[macros(primary_key)]`
  generate a named `ReviewPrimaryKey` struct, not a tuple.

**`authors.rs` and `books.rs` are a deliberate pair, not two ways of doing the same
thing.** `Author` derives `BasicCrudRoutes`, whose `CreateRoute` is bound
`InsertRecord + Deserialize` and deserializes the whole record — so `POST /authors` carries
an explicit `id`, and the caller decides the key. `Book`'s create side is a separate type,
`NewBook`, with no `id` field at all, so the database assigns one on `INSERT ... RETURNING`.
Bundling `CreateRoute` onto `Book` itself was never an option once an assigned key was
wanted: the bundle's create route has no way to take a body that omits the primary key.

The two `UpdateRecord` impls are the other half of that pairing, and `sql_traits`
documents both shapes so this sample exercises each once. `authors.rs`'s `Author` uses
fetch-apply-replace: fetch the row, call the generated `AuthorUpdate::apply`, hand the
result to `ReplaceRecord`. It costs a round trip but reads as three lines. `books.rs`'s
`Book` uses `sqlx::QueryBuilder`, pushing one binding per field that is `Some` into a single
dynamically built `UPDATE`. It touches the database once, at the cost of hand-writing the
statement. Neither handles the empty-update case: `axum_helpers::UpdateRoute` rejects an
empty body with `400` before either impl is called.

## `Genre` is `#[serde(try_from = "String")]`, because a derived `Deserialize` would silently narrow what the wire accepts

`Genre` is a `str_enum!` enum with aliases (`NonFiction` also accepts `"Nonfiction"`,
`ScienceFiction` also accepts `"SciFi"`) and case/separator-insensitive matching. A plain
`#[derive(Deserialize)]` on that enum reads only `#[serde(rename = "...")]`: it would accept
the three canonical strings and reject every alias, silently narrowing what the API takes
on the wire relative to what `Genre::from_str` already accepts everywhere else in the
program. `#[serde(try_from = "String")]` routes deserialization through the `TryFrom<String>`
`str_enum!` generates, which shares its matching with `FromStr`, so the two cannot diverge.

This is not a gap the sample found — `CHANGELOG.md` already records that `str_enum!`
generates `TryFrom<&str>`/`TryFrom<String>` specifically so an enum can be used from
`#[serde(try_from = "String")]` and other `TryFrom`-bounded positions `FromStr` cannot
reach. `books.rs` is simply the first place that documented pairing is exercised
end-to-end, over real JSON, against a real column.

## The database is in-memory and every run re-seeds it from scratch

`connect_and_seed` builds a fresh `sqlite::memory:` pool and applies the schema and seed
rows unconditionally on every `cargo run`. That buys three things at once: the ids
`requests.http` references (`GET /authors/1`, `GET /books/1/reviews/ada`, ...) are always
valid, because they are assigned by the seed rather than by whatever a previous run left
behind; there is nothing to clean up between runs, because a restart is a clean slate; and
there is no stale-schema footgun, because a schema edit here can never disagree with rows
a previous binary already wrote to a file that outlived it. The cost is that anything
created or changed through the API is gone on restart — a deliberate trade, since this
crate exists to inspect a change rather than to accumulate data.

## Static segments are declared beside parameter segments, and axum resolves the conflict by always preferring the static one

`/books/page` and `/books/cursor` sit next to `/books/{book_id}`; `/books/{book_id}/reviews/page`
sits next to `/books/{book_id}/reviews/{reviewer}`. axum's router prefers a static segment
over a parameter at the same position, so `GET /books/page` reaches the paginated-list
handler rather than being parsed as `book_id = "page"`. The cost is symmetric and
documented rather than fixed: a review whose `reviewer` value is literally `"page"` is
unreachable through `GET /books/{book_id}/reviews/{reviewer}`, because `/reviews/page`
always resolves to the paginated-list route first. Any route layout that mixes a literal
segment with a parameter at the same position inherits this; it is a property of the
router, not a bug in this sample.

## Path parameters are named after the field they bind, not uniformly `{id}` — because `Review`'s composite key must be, and `Author`/`Book` follow suit for consistency

`Author` and `Book` mount their single-record routes as `/authors/{author_id}` and
`/books/{book_id}`, not `/authors/{id}` / `/books/{id}`. Both have a scalar `i64` primary
key, so `axum::extract::Path<i64>` binds whatever the segment is named purely by position —
the name is free to choose, and axum 0.8's router (`matchit` 0.8.4, which normalizes
parameter names before inserting a route into its tree, per its own `tree.rs`) does not
care whether it agrees with a differently-named parameter at the same position on another
route. Nothing here would have broken had these stayed `{id}`. They are named
`{author_id}`/`{book_id}` anyway, for readability: both sit under routes that also carry
`/authors/{author_id}/books` and `/books/{book_id}/reviews/...`, so naming the segment
after the column it binds means a reader scanning the route table sees the same name mean
the same book or author everywhere, rather than `{id}` in one route and `{book_id}` two
segments later for what the URL makes the same value.

`Review`'s composite key does not have this freedom. `ReviewPrimaryKey { book_id,
reviewer }` is a named struct, and `axum::extract::Path` fills a struct by matching field
names against segment names — not by position. The route
`/books/{book_id}/reviews/{reviewer}` has to name its segments exactly `book_id` and
`reviewer`; declaring them in the other order changes nothing, but renaming either one
turns the corresponding key field into a `400` from the extractor before the handler runs,
since nothing in the URL would bind it.

## No tests, on purpose

Building this crate under `cargo test --workspace` *is* its assertion — the crate having
no unit tests, integration tests or doctests of its own is not an oversight, it is the
point made in the first section. Response-code coverage for the route traits it uses
already exists in `axum_helpers/tests/route_responses.rs`, against fixtures built for
exactly that purpose; duplicating it here would test the library a second time instead of
testing that the library is usable.

## `cargo check` does not exercise this crate's most interesting failure mode

`books.rs` mounts `CursorParamsQuery<i64, 10, 50>` — a non-default page-size policy on a
const-generic query type whose `POLICY_IS_COHERENT` assertion is evaluated by
monomorphization, not by name resolution or type-checking. `cargo check` type-checks the
generic code without ever instantiating those constants, so a transposed `<i64, 50, 10>` (a
default that exceeds the maximum) would look entirely fine to `cargo check` and to an
editor's inline diagnostics, and would only fail under a real `cargo build` or `cargo
test`. This is the same reason the root `CLAUDE.md` says `cargo check` cannot stand in for
`cargo test` across the workspace; this crate is simply the one place in the repo where
that policy is actually mounted on a route rather than only asserted in a test.
