//! The derive macros behind `axum_helpers`' route traits.
//!
//! Private plumbing: consumers reach these derives through `axum_helpers`, never by
//! declaring this crate. It depends on nothing in this workspace — including
//! `axum_helpers` itself — which is what makes the absolute paths its output emits
//! testable from the outside rather than resolvable by accident.
//!
//! Every path this crate emits is rooted at `::axum_helpers::`, reaching sql and serde
//! items through `::axum_helpers::sql_traits::` and `::axum_helpers::serde::` rather than
//! naming those crates directly. `every_emitted_path_anchors_at_axum_helpers` checks the
//! three derives whose path is hardcoded inside an expander body (`CreateRoute`,
//! `BulkCreateRoute`, `BasicCrudRoutes`); the other six marker derives pass their trait path
//! in from the `#[proc_macro_derive]` entry point, invisible to that test, so
//! `axum_helpers/tests/derive_macros.rs` is what actually proves those six resolve.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{DeriveInput, Ident};

/// Emits `impl <trait_path> for <name> {}` for a route trait whose requirements are
/// enforced by the trait declaration itself, so the impl body is empty.
fn marker_impl(name: &Ident, trait_path: TokenStream2) -> TokenStream2 {
    quote! { impl #trait_path for #name {} }
}

/// Emits the impl for an insert-style route trait. These carry a `'de` lifetime and require
/// their SQL trait's `ReturnType` to be `Serialize`, so they cannot use `marker_impl`.
fn insert_route_impl(name: &Ident, route: TokenStream2, sql_trait: TokenStream2) -> TokenStream2 {
    quote! {
        impl<'de> ::axum_helpers::#route<'de> for #name
        where
            <#name as ::axum_helpers::sql_traits::#sql_trait>::ReturnType:
                ::axum_helpers::serde::Serialize {}
    }
}

/// Body shared by the standalone marker derives.
fn expand_marker(input: TokenStream2, trait_path: TokenStream2) -> TokenStream2 {
    match syn::parse2::<DeriveInput>(input) {
        Ok(input) => marker_impl(&input.ident, trait_path),
        Err(error) => error.to_compile_error(),
    }
}

/// Body shared by the standalone insert-route derives.
fn expand_insert_route(
    input: TokenStream2,
    route: TokenStream2,
    sql_trait: TokenStream2,
) -> TokenStream2 {
    match syn::parse2::<DeriveInput>(input) {
        Ok(input) => insert_route_impl(&input.ident, route, sql_trait),
        Err(error) => error.to_compile_error(),
    }
}

/// Body of the `BasicCrudRoutes` derive. Built from the same emitters as the standalone
/// derives, so the bundle and the individual macros cannot drift.
fn expand_basic_crud_routes(input: TokenStream2) -> TokenStream2 {
    let input: DeriveInput = match syn::parse2(input) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };
    let name = &input.ident;

    let markers = [
        quote! { ::axum_helpers::GetRecordRoute },
        quote! { ::axum_helpers::ListRecordsRoute },
        quote! { ::axum_helpers::ReplaceRoute },
        quote! { ::axum_helpers::UpdateRoute },
        quote! { ::axum_helpers::DeleteRoute },
    ]
    .into_iter()
    .map(|trait_path| marker_impl(name, trait_path));

    let create = insert_route_impl(name, quote!(CreateRoute), quote!(InsertRecord));
    let bulk_create = insert_route_impl(name, quote!(BulkCreateRoute), quote!(BulkInsertRecords));

    quote! {
        #( #markers )*
        #create
        #bulk_create
    }
}

/// Derive `DeleteRoute` (requires the type to implement DeleteRecord + HasPrimaryKey).
#[proc_macro_derive(DeleteRoute)]
pub fn derive_delete_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::DeleteRoute }).into()
}

/// Derive `CreateRoute` (requires the type to implement InsertRecord + Deserialize).
#[proc_macro_derive(CreateRoute)]
pub fn derive_create_route(input: TokenStream) -> TokenStream {
    expand_insert_route(input.into(), quote!(CreateRoute), quote!(InsertRecord)).into()
}

/// Derive `BulkCreateRoute` (requires the type to implement BulkInsertRecords + Deserialize).
#[proc_macro_derive(BulkCreateRoute)]
pub fn derive_bulk_create_route(input: TokenStream) -> TokenStream {
    expand_insert_route(
        input.into(),
        quote!(BulkCreateRoute),
        quote!(BulkInsertRecords),
    )
    .into()
}

/// Derive `GetLatestRoute` (requires the type to implement GetLatestRecord + Serialize).
#[proc_macro_derive(GetLatestRoute)]
pub fn derive_get_latest_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::GetLatestRoute }).into()
}

/// Derive `GetRecordRoute` (requires the type to implement GetRecord + HasPrimaryKey, with a
/// `PrimaryKey` that is `DeserializeOwned`).
#[proc_macro_derive(GetRecordRoute)]
pub fn derive_get_record_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::GetRecordRoute }).into()
}

/// Derive `ListRecordsRoute` (requires the type to implement ListRecords + Serialize).
#[proc_macro_derive(ListRecordsRoute)]
pub fn derive_list_records_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::ListRecordsRoute }).into()
}

/// Derive `ReplaceRoute` (requires the type to implement `ReplaceRecord` + `HasRequestBody`,
/// with a `DeserializeOwned` `PrimaryKey` and `RequestBody`).
///
/// `BasicCrudRoutes` bundles this one, so a type deriving that does not need this as well.
/// Reach for it on a type that wants replace without the rest of CRUD.
#[proc_macro_derive(ReplaceRoute)]
pub fn derive_replace_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::ReplaceRoute }).into()
}

/// Derive `UpdateRoute` (requires the type to implement `UpdateRecord` + `HasUpdateFields`,
/// with a `DeserializeOwned` `PrimaryKey` and `UpdateFields`).
///
/// `BasicCrudRoutes` bundles this one, so a type deriving that does not need this as well.
/// Reach for it on a type that wants partial updates without the rest of CRUD.
#[proc_macro_derive(UpdateRoute)]
pub fn derive_update_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::UpdateRoute }).into()
}

/// Derive `BasicCrudRoutes` — implements every primary-key CRUD route trait for a type:
/// `CreateRoute` and `BulkCreateRoute` (create), `GetRecordRoute` and `ListRecordsRoute`
/// (read), `ReplaceRoute` and `UpdateRoute` (update), and `DeleteRoute` (delete).
///
/// Each of those route traits has supertraits, so the deriving type must implement all of
/// `HasPrimaryKey` (with a `DeserializeOwned` `PrimaryKey`), `HasRequestBody` and
/// `HasUpdateFields` (with `DeserializeOwned` associated types), `GetRecord`, `ListRecords`,
/// `InsertRecord`, `BulkInsertRecords`, `ReplaceRecord`, `UpdateRecord`, and `DeleteRecord`,
/// plus `Serialize` and `Deserialize`, with both insert traits' `ReturnType` also
/// `Serialize`. A missing one is an error on the generated impl naming the trait, not on the
/// derive.
///
/// In practice that means deriving `Record` and `Update` alongside it: those supply
/// `HasPrimaryKey`, `HasRequestBody` and `HasUpdateFields`, and generate the body and update
/// types the two write routes take as request bodies.
///
/// `GetLatestRoute` is deliberately excluded. "The most recent row" is a domain-specific
/// query rather than a CRUD operation, and bundling it would force every deriving type to
/// implement `GetLatestRecord` whether or not it has a meaningful notion of "latest".
#[proc_macro_derive(BasicCrudRoutes)]
pub fn derive_basic_crud_routes(input: TokenStream) -> TokenStream {
    expand_basic_crud_routes(input.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_derives_emit_an_empty_impl_for_the_named_trait() {
        let out = expand_marker(
            "struct Widget { id: i64 }".parse().unwrap(),
            quote! { ::axum_helpers::GetRecordRoute },
        )
        .to_string();
        assert_eq!(out, "impl :: axum_helpers :: GetRecordRoute for Widget { }");
    }

    #[test]
    fn insert_route_derives_bound_the_sql_traits_return_type() {
        let out = expand_insert_route(
            "struct Widget { id: i64 }".parse().unwrap(),
            quote!(CreateRoute),
            quote!(InsertRecord),
        )
        .to_string();
        assert!(
            out.contains("impl < 'de > :: axum_helpers :: CreateRoute < 'de > for Widget"),
            "{out}"
        );
        assert!(out.contains("InsertRecord > :: ReturnType"), "{out}");
    }

    #[test]
    fn basic_crud_routes_emits_every_route_impl() {
        let out =
            expand_basic_crud_routes("struct Widget { id: i64 }".parse().unwrap()).to_string();
        for expected in [
            "GetRecordRoute for Widget",
            "ListRecordsRoute for Widget",
            "DeleteRoute for Widget",
            "ReplaceRoute for Widget",
            "UpdateRoute for Widget",
            "CreateRoute < 'de > for Widget",
            "BulkCreateRoute < 'de > for Widget",
        ] {
            assert!(out.contains(expected), "missing {expected} in {out}");
        }
    }

    #[test]
    fn basic_crud_routes_does_not_emit_get_latest_route() {
        let out =
            expand_basic_crud_routes("struct Widget { id: i64 }".parse().unwrap()).to_string();
        assert!(!out.contains("GetLatestRoute"), "{out}");
    }

    /// Asserts every mention of a `foreign` crate in `out` is reached through `parent`.
    ///
    /// `quote!` renders token streams space-separated, so a path segment appears as
    /// `:: name ::`. A foreign crate is allowed only where the text immediately before it is
    /// `:: <parent>` — that is, `:: axum_helpers :: sql_traits ::` passes and a bare
    /// `:: sql_traits ::` fails. This is what convention 2 asserts in prose.
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
    fn every_emitted_path_anchors_at_axum_helpers() {
        const FOREIGN: &[&str] = &[
            "sql_traits",
            "serde",
            "serde_json",
            "sqlx",
            "async_trait",
            "chrono",
            "generic_helpers",
        ];
        let widget = "struct Widget { id: i64 }";
        let outputs = [
            expand_marker(
                widget.parse().unwrap(),
                quote! { ::axum_helpers::GetRecordRoute },
            )
            .to_string(),
            expand_insert_route(
                widget.parse().unwrap(),
                quote!(CreateRoute),
                quote!(InsertRecord),
            )
            .to_string(),
            expand_basic_crud_routes(widget.parse().unwrap()).to_string(),
        ];
        for out in outputs {
            assert_anchored(&out, "axum_helpers", FOREIGN);
        }
    }
}
