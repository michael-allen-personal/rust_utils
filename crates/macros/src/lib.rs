//! Derive macros for the `sql_traits` and `axum_helpers` route traits.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Fields, Ident, Type};

/// Helper: check if a field has `#[macros(primary_key)]`.
fn is_pk_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        // attribute path starts with `macros`
        if !attr.path().is_ident("macros") {
            return false;
        }
        // Expect something like #[macros(primary_key)]
        match attr.parse_args::<Ident>() {
            Ok(ident) => ident == "primary_key",
            Err(_) => false,
        }
    })
}

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

/// Body of the `PrimaryKey` derive.
/// - 0 fields  -> compile error.
/// - 1 field   -> PrimaryKey = <field_type>
/// - N fields  -> PrimaryKey = (<t1, t2, ...>) in the order encountered.
fn expand_primary_key(input: TokenStream2) -> TokenStream2 {
    let input: DeriveInput = match syn::parse2(input) {
        Ok(parsed) => parsed,
        Err(error) => return error.to_compile_error(),
    };

    let name = &input.ident;

    // Collect marked fields
    let mut pk_types: Vec<Type> = Vec::new();

    match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(named) => {
                for field in &named.named {
                    if is_pk_attr(&field.attrs) {
                        pk_types.push(field.ty.clone());
                    }
                }
            }
            Fields::Unnamed(unnamed) => {
                for field in &unnamed.unnamed {
                    if is_pk_attr(&field.attrs) {
                        pk_types.push(field.ty.clone());
                    }
                }
            }
            Fields::Unit => {}
        },
        _ => {
            return syn::Error::new_spanned(&input, "PrimaryKey can only be derived for structs")
                .to_compile_error();
        }
    }

    if pk_types.is_empty() {
        return syn::Error::new_spanned(
            &input,
            "No field marked with #[macros(primary_key)]. Mark one or more fields.",
        )
        .to_compile_error();
    }

    // TODO: Make this a struct, not a tuple so i can deserialize named values
    // in paths
    // Build the PrimaryKey associated type
    let pk_type_tokens = if pk_types.len() == 1 {
        let t = &pk_types[0];
        quote! { #t }
    } else {
        quote! { ( #( #pk_types ),* ) }
    };

    quote! {
        impl ::sql_traits::HasPrimaryKey for #name {
            type PrimaryKey = #pk_type_tokens;
        }
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
        quote! { ::axum_helpers::GetLatestRoute },
        quote! { ::axum_helpers::GetRecordRoute },
        quote! { ::axum_helpers::ListRecordsRoute },
        quote! { ::axum_helpers::DeleteRoute },
    ]
    .into_iter()
    .map(|trait_path| marker_impl(name, trait_path));

    let create = insert_route_impl(name, quote!(CreateRoute), quote!(InsertSQL));
    let bulk_create = insert_route_impl(name, quote!(BulkCreateRoute), quote!(BulkInsertSQL));

    quote! {
        #( #markers )*
        #create
        #bulk_create
    }
}

/// Derive `HasPrimaryKey` by inspecting fields marked with #[macros(primary_key)].
#[proc_macro_derive(PrimaryKey, attributes(macros))]
pub fn derive_primary_key(input: TokenStream) -> TokenStream {
    expand_primary_key(input.into()).into()
}

/// Derive `DeleteRoute` (requires the type to implement DeleteSQL + HasPrimaryKey).
#[proc_macro_derive(DeleteRoute)]
pub fn derive_delete_route(input: TokenStream) -> TokenStream {
    expand_marker(input.into(), quote! { ::axum_helpers::DeleteRoute }).into()
}

/// Derive `CreateRoute` (requires the type to implement InsertSQL + Deserialize).
#[proc_macro_derive(CreateRoute)]
pub fn derive_create_route(input: TokenStream) -> TokenStream {
    expand_insert_route(input.into(), quote!(CreateRoute), quote!(InsertSQL)).into()
}

/// Derive `BulkCreateRoute` (requires the type to implement BulkInsertSQL + Deserialize).
#[proc_macro_derive(BulkCreateRoute)]
pub fn derive_bulk_create_route(input: TokenStream) -> TokenStream {
    expand_insert_route(input.into(), quote!(BulkCreateRoute), quote!(BulkInsertSQL)).into()
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

/// Derive `BasicCrudRoutes` — implements all route traits for a type: `GetLatestRoute`,
/// `GetRecordRoute`, `ListRecordsRoute`, `DeleteRoute`, `CreateRoute`, and `BulkCreateRoute`.
///
/// Note this requires `GetRecord`, which `GetRecordRoute` takes as a supertrait.
#[proc_macro_derive(BasicCrudRoutes)]
pub fn derive_basic_crud_routes(input: TokenStream) -> TokenStream {
    expand_basic_crud_routes(input.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(src: &str) -> String {
        expand_primary_key(src.parse().unwrap()).to_string()
    }

    #[test]
    fn single_marked_field_becomes_the_primary_key_type() {
        let out = expand("struct User { #[macros(primary_key)] id: i64, name: String }");
        assert!(out.contains("type PrimaryKey = i64"), "{out}");
    }

    #[test]
    fn multiple_marked_fields_become_a_tuple_in_declaration_order() {
        let out = expand(
            "struct Membership { #[macros(primary_key)] user_id: i64, #[macros(primary_key)] group_id: u32 }",
        );
        assert!(out.contains("type PrimaryKey = (i64 , u32)"), "{out}");
    }

    #[test]
    fn unmarked_struct_is_a_compile_error() {
        let out = expand("struct User { id: i64 }");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("No field marked"), "{out}");
    }

    #[test]
    fn enum_is_a_compile_error() {
        let out = expand("enum Color { Red }");
        assert!(out.contains("can only be derived for structs"), "{out}");
    }

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
            quote!(InsertSQL),
        )
        .to_string();
        assert!(
            out.contains("impl < 'de > :: axum_helpers :: CreateRoute < 'de > for Widget"),
            "{out}"
        );
        assert!(out.contains("InsertSQL > :: ReturnType"), "{out}");
    }

    #[test]
    fn basic_crud_routes_emits_every_route_impl() {
        let out =
            expand_basic_crud_routes("struct Widget { id: i64 }".parse().unwrap()).to_string();
        for expected in [
            "GetLatestRoute for Widget",
            "GetRecordRoute for Widget",
            "ListRecordsRoute for Widget",
            "DeleteRoute for Widget",
            "CreateRoute < 'de > for Widget",
            "BulkCreateRoute < 'de > for Widget",
        ] {
            assert!(out.contains(expected), "missing {expected} in {out}");
        }
    }
}
