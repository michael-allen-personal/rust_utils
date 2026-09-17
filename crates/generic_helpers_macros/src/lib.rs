//! The derive macro behind `generic_helpers::MaxVecCapacity`.
//!
//! Private plumbing: consumers reach this derive through `generic_helpers`, never by
//! declaring this crate. It depends on nothing in this workspace — including
//! `generic_helpers` itself — which is what makes the absolute path its output emits
//! testable from the outside rather than resolvable by accident.
//!
//! `every_emitted_path_anchors_at_generic_helpers` passes this derive's own trait path in as
//! an argument, the same way the real entry point does, so it cannot catch a wrong path
//! here — only a path hardcoded inside an expander body is visible to that kind of test.
//! `generic_helpers/tests/derive_macros.rs` is what actually proves
//! `::generic_helpers::MaxVecCapacity` resolves, by linking the derive from outside.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, Ident};

/// Emits `impl <trait_path> for <name> {}` for a trait whose requirements are enforced by
/// the trait declaration itself, so the impl body is empty.
fn marker_impl(name: &Ident, trait_path: TokenStream2) -> TokenStream2 {
    quote! { impl #trait_path for #name {} }
}

/// Body shared by every marker derive. Kept here rather than in a crate shared with
/// `axum_helpers_macros`: ten lines duplicated cost less than a fourth crate, and the two
/// copies cannot drift in any way that matters because each is exercised by its own crate's
/// tests.
fn expand_marker(input: TokenStream2, trait_path: TokenStream2) -> TokenStream2 {
    match syn::parse2::<DeriveInput>(input) {
        Ok(input) => marker_impl(&input.ident, trait_path),
        Err(error) => error.to_compile_error(),
    }
}

/// Derive `MaxVecCapacity` — a marker impl opting a type into the trait's provided
/// `estimate_max_vec_capacity_from_file`.
///
/// The emitted path is absolute and aimed at `generic_helpers`' root re-export; both are
/// load-bearing, and `generic_helpers/CLAUDE.md` records why. That crate's
/// `tests/derive_macros.rs` is the consumer-perspective compile test that fails if the
/// path stops resolving from outside the crate.
#[proc_macro_derive(MaxVecCapacity)]
pub fn derive_max_vec_capacity(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::generic_helpers::MaxVecCapacity }).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_derive_emits_an_empty_impl_for_the_named_trait() {
        let out = expand_marker(
            "struct Rows { id: i64 }".parse().unwrap(),
            quote! { ::generic_helpers::MaxVecCapacity },
        )
        .to_string();
        assert_eq!(
            out,
            "impl :: generic_helpers :: MaxVecCapacity for Rows { }"
        );
    }

    /// Asserts every mention of a `foreign` crate in `out` is reached through `parent`.
    ///
    /// `quote!` renders token streams space-separated, so a path segment appears as
    /// `:: name ::`. A foreign crate is allowed only where the text immediately before it is
    /// `:: <parent>` — that is, `:: generic_helpers :: error_set ::` passes and a bare
    /// `:: error_set ::` fails. This is what convention 2 asserts in prose.
    fn assert_anchored(out: &str, parent: &str, foreign: &[&str]) {
        let through = format!(":: {parent}");
        for name in foreign {
            let needle = format!(":: {name} ::");
            let mut from = 0;
            while let Some(offset) = out[from..].find(&needle) {
                let at = from + offset;
                assert!(
                    out[..at].trim_end().ends_with(&through),
                    "`{name}` is named without going through `{parent}`:\n{out}"
                );
                from = at + needle.len();
            }
        }
    }

    #[test]
    fn every_emitted_path_anchors_at_generic_helpers() {
        const FOREIGN: &[&str] = &["sql_traits", "axum_helpers", "serde", "sqlx", "error_set"];
        let out = expand_marker(
            "struct Rows { id: i64 }".parse().unwrap(),
            quote! { ::generic_helpers::MaxVecCapacity },
        )
        .to_string();
        assert_anchored(&out, "generic_helpers", FOREIGN);
    }
}
