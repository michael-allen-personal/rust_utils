# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

A workspace of four small library crates, consumed by other repos. There is no binary and
no application here — every crate is a set of traits, macros, or helpers that something
else implements. That shapes almost everything below: the interesting failures are things
that compile *here* and break at a use site.

## Commands

```bash
cargo test                                  # unit + integration + doctests, whole workspace
cargo test -p sql_traits                    # one crate
cargo test -p axum_helpers --test route_responses   # one integration target
cargo test -p macros single_marked_field    # one test, by name substring
cargo clippy --all-targets
cargo fmt --check
```

`--all-targets` and `cargo test` are not interchangeable with `cargo check`. Most of this
workspace's assertions live in `tests/` crates that a plain `cargo check` never builds, so
a bare `cargo check` can pass while the generated macro output is broken. Run `cargo test`
before believing a change works.

No database is needed. `sqlx::PgPool::connect_lazy` builds a pool without connecting, and
the test fixtures' SQL impls ignore it, so handlers can be driven without Postgres.

## The crates

```
generic_helpers      standalone; str_enum! + file helpers. One dep (error_set).
sql_traits           database traits over sqlx/Postgres. Knows nothing about HTTP.
axum_helpers    ───▶ sql_traits. Wraps each SQL trait in an axum route handler.
macros               proc-macro crate. Depends on NONE of the above.
```

The layering is one-way and worth preserving: `sql_traits` must stay free of `axum`, so a
non-HTTP consumer can use the database traits alone.

## Three workspace-wide conventions

These are the rules that generated a bug the last time each was broken, and they are the
reason the crates are shaped the way they are.

### 1. Leaked-type dependencies are re-exported

A dependency whose types appear in a public signature is re-exported (`sql_traits::sqlx`,
`axum_helpers::axum`, and so on). Consumers depend on the re-export rather than declaring
the crate themselves, which guarantees a single compiled copy — two incompatible `sqlx`
versions otherwise produce "expected `Pool`, found `Pool`". Adding a dependency that shows
up in a signature means adding a `pub use` for it too.

### 2. Macro output names everything by absolute path

`macros` depends on none of the crates it generates code for. It emits
`::sql_traits::HasPrimaryKey`, `::axum_helpers::UpdateRoute`,
`::generic_helpers::MaxVecCapacity` — paths that resolve only at the *use* site, through
the re-exports from rule 1. Two consequences:

- A bare `sql_traits::` in an expansion resolves in the caller's module. It works
  everywhere it is tested in-repo and breaks for any consumer with a local item of that
  name. Same for `macro_rules!`: `str_enum!` uses `$crate::` and `::core::` throughout.
- Crate names and root re-export positions are load-bearing across repos. Renaming
  `generic_helpers`, or moving `MaxVecCapacity` out of its crate root, breaks every derive
  site with an error pointing at the derive rather than at the change.

The `MaxVecCapacity` derive was deleted outright in v0.6.0 rather than fixed, because it
emitted a path to a crate in a different repo and nothing here could compile it.

### 3. Tests are separate crates that consume the library

Files under `tests/` compile as their own crates linking the library externally, which is
the only vantage point from which a wrong path, a missing `#[macro_export]`, or an
unmet trait bound in generated code is visible. An in-crate `#[cfg(test)] mod tests`
resolves everything locally and proves nothing about the external contract.

Each test crate's dependencies are deliberately minimal: they reach `serde`, `sqlx` and
`sql_traits` only through the crate under test's re-exports, so the build failing *is* the
assertion. The `[dev-dependencies]` blocks carry comments explaining why each entry is
there — read them before adding one.

The single exception is `macros`, whose tests are in-crate because they assert on the
token strings its private expansion functions return. Its consumer-perspective coverage
lives in the other crates' `tests/derive_macros.rs`.

## Changelog and versions

`CHANGELOG.md` is maintained per change under `## Unreleased`, with Keep-a-Changelog
subsections (`### Added` / `### Changed` / `### Removed`) and a **Breaking.** prefix on
entries that require downstream edits. Entries here are long-form: they say what changed
*and* why, and what a consumer has to do about it. Match that.

Releases are tagged `vX.Y.Z` across the whole workspace; the per-crate `version` fields in
`Cargo.toml` have all stayed at `0.1.0`.
