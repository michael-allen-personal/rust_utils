use proc_macro::TokenStream;
use quote::quote;
use syn::{Attribute, Data, DeriveInput, Fields, Ident, Type, parse_macro_input};

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

/// Derive `HasPrimaryKey` by inspecting fields marked with #[macros(primary_key)].
/// - 0 fields  -> compile error.
/// - 1 field   -> PrimaryKey = <field_type>
/// - N fields  -> PrimaryKey = (<t1, t2, ...>) in the order encountered.
#[proc_macro_derive(PrimaryKey, attributes(macros))]
pub fn derive_primary_key(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);

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
                .to_compile_error()
                .into();
        }
    }

    if pk_types.is_empty() {
        return syn::Error::new_spanned(
            &input,
            "No field marked with #[macros(primary_key)]. Mark one or more fields.",
        )
        .to_compile_error()
        .into();
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

    let expanded = quote! {
        impl sql_traits::HasPrimaryKey for #name {
            type PrimaryKey = #pk_type_tokens;
        }
    };

    TokenStream::from(expanded)
}

/// Derive `DeleteRoute` (requires the type to implement DeleteSQL + HasPrimaryKey).
/// This macro just emits `impl axum_helpers::DeleteRoute for Type {}`.
#[proc_macro_derive(DeleteRoute)]
pub fn derive_delete_route(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    let expanded = quote! {
        impl axum_helpers::DeleteRoute for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive `CreateRoute` similarly: `impl axum_helpers::CreateRoute<'de> for Type {}`
/// We use a named lifetime `'de` because your trait has one.
#[proc_macro_derive(CreateRoute)]
pub fn derive_create_route(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    // Note: we don't need to mention T: Deserialize<'de> here; your trait bound
    // enforces it at use sites. If you want a nicer error, you can add a where clause.
    let expanded = quote! {
        impl<'de> axum_helpers::CreateRoute<'de> for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive `BulkCreateRoute` similarly: `impl axum_helpers::BulkCreateRoute<'de> for Type {}`
/// We use a named lifetime `'de` because your trait has one.
#[proc_macro_derive(BulkCreateRoute)]
pub fn derive_bulk_create_route(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    // Note: we don't need to mention T: Deserialize<'de> here; your trait bound
    // enforces it at use sites. If you want a nicer error, you can add a where clause.
    let expanded = quote! {
        impl<'de> axum_helpers::BulkCreateRoute<'de> for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive `GetLatestRoute` similarly: `impl axum_helpers::GetLatestRoute for Type {}`
#[proc_macro_derive(GetLatestRoute)]
pub fn derive_get_latest_route(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    let expanded = quote! {
        impl axum_helpers::GetLatestRoute for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive `ListRecordsRoute`: `impl axum_helpers::ListRecordsRoute for Type {}`
#[proc_macro_derive(ListRecordsRoute)]
pub fn derive_list_records_route(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    let expanded = quote! {
        impl axum_helpers::ListRecordsRoute for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive `BasicCrudRoutes` — implements all route traits for a type:
/// `GetLatestRoute`, `ListRecordsRoute`, `CreateRoute`, `BulkCreateRoute`, `DeleteRoute`
#[proc_macro_derive(BasicCrudRoutes)]
pub fn derive_basic_crud_routes(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    let expanded = quote! {
        impl axum_helpers::GetLatestRoute for #name {}
        impl axum_helpers::ListRecordsRoute for #name {}
        impl<'de> axum_helpers::CreateRoute<'de> for #name {}
        impl<'de> axum_helpers::BulkCreateRoute<'de> for #name {}
        impl axum_helpers::DeleteRoute for #name {}
    };

    TokenStream::from(expanded)
}

/// Derive `MaxVecCapacity` similarly:
/// `impl common_parser::MaxVecCapacity for Type {}`
#[proc_macro_derive(MaxVecCapacity)]
pub fn max_vec_capacity_derive(input: TokenStream) -> TokenStream {
    let input: DeriveInput = parse_macro_input!(input);
    let name = &input.ident;

    let impl_block = quote! {
        impl common_parser::MaxVecCapacity for #name {}
    };

    TokenStream::from(impl_block)
}
