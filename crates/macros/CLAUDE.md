# macros

Every derive in the workspace. Thirteen of them, in three tiers.

**This crate depends on nothing it generates code for** — only `syn`, `quote` and
`proc-macro2`. Nothing here can type-check its own output; the expansions name
`::sql_traits::…`, `::axum_helpers::…` and `::generic_helpers::…`, and those resolve only at
a use site, through the target crates' re-exports. That single fact drives everything below.

## The three tiers

**Generators** — `PrimaryKey`, `Record`, `Update`. These read `#[macros(...)]` and emit new
types plus the `sql_traits` association impls. Reserved names: `{Name}PrimaryKey`,
`{Name}Body`, `{Name}Update`.

**Route markers** — `GetRecordRoute`, `ListRecordsRoute`, `ReplaceRoute`, `UpdateRoute`,
`DeleteRoute`, `GetLatestRoute` (empty impls, requirements enforced by the trait
declaration), and `CreateRoute` / `BulkCreateRoute` (a separate emitter: they carry a `'de`
lifetime and require their SQL trait's `ReturnType: Serialize`).

**Bundle** — `BasicCrudRoutes`, built from the same two emitters as the standalone derives so
the bundle and the individual macros cannot drift. It excludes `GetLatestRoute` on purpose:
"the most recent row" is a domain query, not a CRUD operation, and bundling it would force
every deriving type to implement `GetLatestRecord`.

Plus `MaxVecCapacity`, which belongs to `generic_helpers` and is otherwise unrelated.

`Record` and `Update` are designed to be derived together. `PrimaryKey` alongside `Record`
is not: both emit `impl HasPrimaryKey`, so it is a pile of duplicate-item errors.

## Absolute paths, always

An unqualified `sql_traits::` or `Option` in an expansion resolves in the *caller's* module.
It compiles in every test in this repo and breaks for any consumer with a local item of that
name. So: `::sql_traits::`, `::core::option::Option`, `::core::convert::From`, and `serde`
reached as `::sql_traits::serde` with `#[serde(crate = "...")]` pointing serde's own
generated code back at the same place.

The `MaxVecCapacity` derive has already been lost to this once — it emitted
`common_parser::MaxVecCapacity`, a crate in a different repo, and was deleted in v0.6.0
rather than fixed. The absolute path plus `generic_helpers`' root re-export are both
load-bearing; `crates/generic_helpers/CLAUDE.md` records the full story.

## `#[macros(...)]` errors, never skips

Exactly three directives: `primary_key` on a field, `body_derive(...)` and
`update_derive(...)` on the struct. Anything else — a typo, a stray comma, a non-list form, a
struct directive written on a field — is a `compile_error!` naming what was found and what is
accepted. Errors are accumulated, so several bad attributes all report at once.

Silence here was actively dangerous, not merely unhelpful: a malformed
`#[macros(primary_key,)]` used to drop the field out of the key *and* put it into the
generated request body — the exact key leak the body type exists to prevent, and one no
round-trip test can catch, because the isomorphism still holds when the partition is wrong.

`Record` and `Update` tolerate *each other's* struct-level list, since a derive macro cannot
see its siblings and so cannot tell a typo from a directive meant for the other one.

## Why derive lists have to be named explicitly

rustc strips sibling `#[derive(...)]` attributes before a derive macro runs, so the derives
for a generated type cannot be copied and must be spelled out in `body_derive` /
`update_derive`. *Other* attributes are forwarded automatically — everything except this
crate's own `#[macros(...)]` — which is what keeps `serde` and `ts-rs` renames aligned
between a record and its body. Note that includes `#[serde(deny_unknown_fields)]`: a record
carrying it yields a body that rejects a payload containing the primary key. That is
deliberate; an explicit opt-in to strictness is honoured rather than quietly overridden.

The one exception is `{Name}PrimaryKey`, which forwards nothing and carries a fixed derive
set (`Clone`, `Debug`, `PartialEq`, `Deserialize`). Its wire format is URL path segments, not
JSON, so a `rename_all` meant for a request body has no business renaming the segments a
route must declare.

## Two decisions to preserve

**A composite key is a named struct, not a tuple.** `axum::extract::Path` fills a tuple from
URI segments left to right with no name matching, so a route declaring its segments in a
different order from the marked fields compiles, mounts, runs — and addresses the wrong row.
A struct binds by name. A single marked field stays its own bare type: one path segment binds
unambiguously, and wrapping it would break every `get_record(&pool, 5)` for nothing. A
composite key on a tuple struct has no field names to bind to, so it is a compile error
rather than a silent fallback.

`PrimaryKeyShape` builds the associated type, the accessor, and the destructuring pattern
together, and is shared by `PrimaryKey` and `Record` — so a key field cannot be rebuilt into
the wrong slot. Keep it that way. It clones marked fields (fully qualified, so clippy's
`clone_on_copy` stays quiet on `i64` keys), which is why they must be `Clone`.

**`is_option` is syntactic and that is fine.** It matches the last path segment, so
`Option<T>`, `std::option::Option<T>` and `::core::option::Option<T>` all count; a type
*alias* for `Option<T>` does not, and no proc macro can resolve one. The blast radius is
contained by design: the generated field still gets the right type, and only loses its
`deserialize_with`, so an explicit `null` reads as "leave alone" rather than "clear". A wrong
answer is never a type error, only a missing capability. Do not try to make it smarter.

## Tests

Unit tests in `src/lib.rs` — the exception to the workspace rule that tests live in `tests/`
(see the root `CLAUDE.md`). They assert on the token strings the private `expand_*` functions
return, which is only reachable in-crate. This is why `proc_macro::TokenStream` appears solely
in the `#[proc_macro_derive]` signatures and every expansion function takes and returns
`proc_macro2::TokenStream`.

Token-string assertions cannot tell you whether the output *resolves*. That half is covered
from outside, by `sql_traits/tests/derive_macros.rs`, `axum_helpers/tests/derive_macros.rs`
and `generic_helpers/tests/derive_macros.rs`. A change to an emitted path needs both.
