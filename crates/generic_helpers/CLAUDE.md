# generic_helpers

Small shared helpers, one dependency (`error_set`). Two unrelated halves:

- **`str_enum!`** — declares a fieldless enum plus its whole string round-trip.
- **File helpers** — `buf_reader_from_path` and `MaxVecCapacity`, for file-parsing
  CLIs.

The name is generic; the admission rule is not. Stay at one dependency and keep
additions applicable to more than one consumer — otherwise it belongs in the
specific crate. Consumers pull this in for one small helper apiece, so anything
heavy lands in their build for no benefit.

## `MaxVecCapacity` and its derive

The file helpers are re-exported at the crate root (`generic_helpers::MaxVecCapacity`),
not under `files::`. Keep it that way: the `MaxVecCapacity` derive in `macros` emits
`::generic_helpers::MaxVecCapacity`, so both the crate name and that root re-export are
load-bearing in every downstream expansion. Renaming either breaks each derive site with
an error that points at the derive, not at the rename.

This derive has been broken once already. It originally emitted a bare
`common_parser::MaxVecCapacity` — a crate in a different repo, unreachable from here —
and was deleted in v0.6.0 rather than fixed, because nothing in this workspace could
even compile it. Two things prevent a repeat, and both matter:

1. **The emitted path is absolute** (`::generic_helpers::…`). A bare `generic_helpers::`
   resolves in the *caller's* module, so it would break for any consumer with a local
   item of that name — and work fine everywhere it was tested.
2. **`tests/derive_macros.rs` is a consumer-perspective compile test.** It depends only
   on this crate and `macros`, and never imports the trait. That is the only vantage
   point from which a wrong path is visible; an in-crate test resolves everything
   locally and proves nothing. Same pattern as `sql_traits/tests/derive_macros.rs`.

## `str_enum!`

```rust
use generic_helpers::str_enum;

str_enum! {
  pub enum ExerciseMechanicsType {
    Compound => "Compound",
    Isolated => "Isolated" | "Isolation",   // aliases parse, never display
  }
}
```

Generates `as_str`, `Display`, `FromStr`, `TryFrom<&str>`, `TryFrom<String>`
(all three parse entry points erroring with `ParsingError`), `ALL`, and
`EXPECTED` — all from one set of literals, so the display string, the error
message, and the parse table cannot drift apart. The `TryFrom` impls exist
because `FromStr` is unreachable from `#[serde(try_from = "String")]` and other
`TryFrom`-bounded positions; both route through the same private `accepts`
helper as `FromStr`, so alias and case handling cannot diverge between them.
Matching ignores case and non-alphanumeric characters, so
`"dynamic-stabilizer"` and `"DynamicStabilizer"` both reach
`DynamicStabilizer` without needing an alias.

The enum always derives `Debug, Clone, Copy, PartialEq, Eq, Hash`; callers must
not repeat those. Attributes pass through on both the enum and its variants,
which is how callers attach `serde`/`ts-rs` without this crate depending on
either. Note that plain `#[derive(Serialize)]` would emit `"DynamicStabilizer"`
while `as_str` says `"Dynamic Stabilizer"` — a per-variant
`#[serde(rename = "...")]` is what keeps the wire format and the display string
aligned.

### Two rules that are easy to regress

1. **Every path in the expansion must be absolute** — `$crate::…` or `::core::…`.
   `macro_rules!` is hygienic for local variables but *not* for paths to items:
   an unqualified `FromStr` or `ParsingError` resolves in the caller's scope, so
   it compiles fine in this crate and breaks for everyone else. `normalized_eq`
   lives behind `$crate::__private` for exactly this reason — it needs an
   absolute path to reach, without becoming public API.

2. **Tests go in `tests/`, never `#[cfg(test)] mod tests`.** Files under `tests/`
   compile as separate crates linking this one externally, which is the only way
   to catch a missing `#[macro_export]` or a leaked unqualified path. An in-crate
   test module resolves every path locally and so proves nothing about the
   macro's external contract. `tests/str_enum.rs` deliberately imports *only* the
   macro; if the expansion ever needs something else in scope, it stops compiling.

The doctests on the macro are a second external check — doctests link the crate
as a consumer too, so they catch the same class of bug.

`ParsingError` is not `PartialEq`, so tests compare `.parse().ok()` against
`Some(variant)` rather than the `Result` directly.
