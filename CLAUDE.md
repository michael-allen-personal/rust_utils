# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A workspace of three small library crates, each paired with a private macro crate behind
it, consumed by other repos, plus one sample application. Every library crate is a set of
traits, macros, or helpers that something else implements; `examples/sample_api` is the one
place in this workspace that plays the role of that "something else." That shapes almost
everything below: the interesting failures are things that compile *here* and break at a
use site — and the sample is now one of the places such a break shows up first, before it
ever reaches a downstream repo.

## Commands

```bash
cargo test                                  # unit + integration + doctests, whole workspace
cargo test -p sql_traits                    # one crate
cargo test -p axum_helpers --test route_responses   # one integration target
cargo test -p sql_traits_macros single_marked_field # one test, by name substring
cargo clippy --all-targets
cargo fmt --check
cargo run -p sample_api                     # the sample API on http://127.0.0.1:3114
```

`--all-targets` and `cargo test` are not interchangeable with `cargo check`. Most of this
workspace's assertions live in `tests/` crates that a plain `cargo check` never builds, so
a bare `cargo check` can pass while the generated macro output is broken. Run `cargo test`
before believing a change works.

No database *server* is needed. Where a test needs a real query, it builds a real in-memory
SQLite database (`sqlite::memory:`) — `sqlx` 0.9 shares one in-memory database across every
connection a pool hands out, so no `max_connections(1)` or shared-cache URI juggling is
required. Fixtures whose subject is HTTP or macro plumbing rather than the SQL itself still
fake their trait impls; either way, nothing here talks to a running Postgres.

## The crates

```
generic_helpers        standalone; str_enum! + file helpers + the MaxVecCapacity derive.
generic_helpers_macros private; the MaxVecCapacity derive. Depends on NONE of the above.
sql_traits             database traits over sqlx, generic over the driver. Knows nothing about HTTP.
sql_traits_macros      private; Record/Update/PrimaryKey/Database. Depends on NONE of the above.
axum_helpers      ───▶ sql_traits. Wraps each SQL trait in an axum route handler.
axum_helpers_macros    private; the route derives. Depends on NONE of the above.
sample_api        ───▶ sql_traits + axum_helpers + generic_helpers. A runnable example.
```

The layering is one-way and worth preserving: `sql_traits` must stay free of `axum`, so a
non-HTTP consumer can use the database traits alone.

Every SQL trait and every route trait is generic over the database via
`sql_traits::HasDatabase` — a record names its database once, as an associated type, and
every pool-taking method and axum mount site follows it rather than each naming `PgPool`
for itself. Neither `sql_traits` nor `axum_helpers` enables a `sqlx` driver or picks a
runtime feature; both are the consumer's choice, made through the
`postgres`/`sqlite`/`mysql`/`any` features each crate forwards to `sqlx`.

## Three workspace-wide conventions

These are the rules that generated a bug the last time each was broken, and they are the
reason the crates are shaped the way they are.

### 1. Leaked-type dependencies are re-exported

A dependency whose types appear in a public signature is re-exported (`sql_traits::sqlx`,
`axum_helpers::axum`, and so on). Consumers depend on the re-export rather than declaring
the crate themselves, which guarantees a single compiled copy — two incompatible `sqlx`
versions otherwise produce "expected `Pool`, found `Pool`". Adding a dependency that shows
up in a signature means adding a `pub use` for it too.

The same reasoning covers driver selection. A consumer depends on the re-exported `sqlx`
rather than a separately declared one, so it has no `sqlx` of its own on which to flip a
driver feature. `sql_traits` and `axum_helpers` therefore each forward
`postgres`/`sqlite`/`mysql`/`any` to their `sqlx` dependency (`axum_helpers`'s forward to
`sql_traits`'s too), so a consumer picks a driver through the crate it already depends on.

There is one exception, and it is deliberate. `sql_traits` is declared directly by any
consumer that derives `sql_traits::Record`, `Update`, `PrimaryKey` or `Database` — the
derive's generated `::sql_traits::…` paths resolve only where the use site has that crate
in its extern prelude, and writing the derive's name is what guarantees it. The
single-compiled-copy property therefore rests on the consumer pinning `sql_traits` and
`axum_helpers` to the same source rather than on there being only one declaration; across
this workspace's consumer repos, pinned to one `vX.Y.Z` tag, that holds by construction.

### 2. Macro output anchors at its own crate

Each of the three private macro crates emits paths rooted only at its parent library's
crate root, plus `::core::`/`::std::` — `sql_traits_macros` reaches `serde` and `sqlx` as
`::sql_traits::serde`/`::sql_traits::sqlx`, `axum_helpers_macros` reaches `sql_traits` and
`serde` as `::axum_helpers::sql_traits`/`::axum_helpers::serde`, and `generic_helpers_macros`
names nothing else at all. Everything foreign is reached through a re-export on that one
root rather than named directly.

- A bare `sql_traits::` in an expansion resolves in the caller's module. It works
  everywhere it is tested in-repo and breaks for any consumer with a local item of that
  name. Same for `macro_rules!`: `str_enum!` uses `$crate::` and `::core::` throughout.
- Crate names and root re-export positions are load-bearing across repos. Renaming
  `generic_helpers`, or moving `MaxVecCapacity` out of its crate root, breaks every derive
  site with an error pointing at the derive rather than at the change.

The `MaxVecCapacity` derive was deleted outright in v0.6.0 rather than fixed, because it
emitted a path to a crate in a different repo and nothing here could compile it.

**The anchor rule is checked by two mechanisms together, not one.** Each macro crate's
`every_emitted_path_anchors_at_*` test only sees paths hardcoded inside an expander body —
it cannot catch a path a `#[proc_macro_derive]` entry point supplies as an argument, since
that argument is exactly what the test itself also supplies. That gap is
`generic_helpers_macros`' one derive and six of `axum_helpers_macros`' marker derives
(`GetRecordRoute`, `ListRecordsRoute`, `ReplaceRoute`, `UpdateRoute`, `DeleteRoute`,
`GetLatestRoute`), which hand their trait path to a shared `expand_marker`. Convention 3's
external `tests/derive_macros.rs` covers exactly that gap: it links the derive from
outside, so an entry point's wrong path fails to resolve at `cargo test` time rather than
passing silently. Together the two mechanisms check the whole rule; neither alone does.

### 3. Tests are separate crates that consume the library

Files under `tests/` compile as their own crates linking the library externally, which is
the only vantage point from which a wrong path, a missing `#[macro_export]`, or an
unmet trait bound in generated code is visible. An in-crate `#[cfg(test)] mod tests`
resolves everything locally and proves nothing about the external contract.

Each test crate's dependencies are deliberately minimal: they reach `serde`, `sqlx` and
`sql_traits` only through the crate under test's re-exports, so the build failing *is* the
assertion. The `[dev-dependencies]` blocks carry comments explaining why each entry is
there — read them before adding one. None of them declares a macro crate any more: the
derives now arrive through the crate under test's own re-export, so `sql_traits_macros`,
`axum_helpers_macros` and `generic_helpers_macros` are never named outside their own crate.
`generic_helpers` needs nothing beyond its own re-export to prove its one derive resolves,
and consequently has no `[dev-dependencies]` section at all.

The exceptions are the three macro crates — `sql_traits_macros`, `axum_helpers_macros` and
`generic_helpers_macros` — whose tests are in-crate because they assert on the token
strings their private expansion functions return, which is only reachable in-crate. Their
consumer-perspective coverage lives in the other crates' `tests/derive_macros.rs`, and
each additionally carries an `every_emitted_path_anchors_at_*` test enforcing the anchor
rule on its own output.

`examples/sample_api` is a fifth vantage point of the same kind, even though it is not
under a `tests/` directory: it reaches `axum`, `serde`, `sqlx` and `sql_traits` only
through `axum_helpers`' re-exports (plus a direct `sql_traits` dependency for the reason
above), so it failing to build is the same class of assertion — the re-export surface
being sufficient to write a real application, not merely to pass a compile-only fixture.

## Changelog and versions

`CHANGELOG.md` is maintained per change under `## Unreleased`, with Keep-a-Changelog
subsections (`### Added` / `### Changed` / `### Removed`) and a **Breaking.** prefix on
entries that require downstream edits. Entries here are long-form: they say what changed
*and* why, and what a consumer has to do about it. Match that.

Releases are tagged `vX.Y.Z` across the whole workspace; the per-crate `version` fields in
`Cargo.toml` have all stayed at `0.1.0`.
