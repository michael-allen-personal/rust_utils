//! Derive macros for the `sql_traits` and `axum_helpers` route traits.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Field, Fields, Ident, Index, Member, Meta, Path, PathArguments,
    Token, Type, Visibility, parse_quote, punctuated::Punctuated,
};

/// The complete list of directives `#[macros(...)]` accepts, named in every error so a
/// typo says what to write instead.
const MACROS_ATTR_HELP: &str = "`#[macros(...)]` accepts exactly `primary_key` on a field \
     and `body_derive(Trait, ...)` or `update_derive(Trait, ...)` on the struct";

/// One recognized `#[macros(...)]` directive.
enum MacrosDirective {
    /// `#[macros(primary_key)]` — this field is (part of) the primary key.
    PrimaryKey,
    /// `#[macros(body_derive(A, B))]` — the derives to put on the generated body type.
    BodyDerive(Vec<Path>),
    /// `#[macros(update_derive(A, B))]` — the derives to put on the generated update type.
    UpdateDerive(Vec<Path>),
}

/// Which struct-level derive list a given derive macro reads.
///
/// A derive macro cannot see its siblings, so it cannot tell whether a directive it does
/// not consume is a typo or is there for another derive on the same struct. `Record` and
/// `Update` are designed to be used together, so each tolerates the other's list rather
/// than rejecting it; a directive neither of them recognizes is still a hard error, caught
/// earlier in `parse_macros_attr`.
#[derive(Clone, Copy)]
enum DeriveList {
    /// `Record`, which reads `body_derive`.
    Body,
    /// `Update`, which reads `update_derive`.
    Update,
    /// `PrimaryKey`, whose only generated type — the key struct for a composite key —
    /// carries a fixed set of derives, so it reads neither list.
    Neither,
}

/// Parses one `#[macros(...)]` attribute, rejecting anything that is not a recognized
/// directive.
///
/// Every failure is a hard error rather than a skip. A silently ignored directive is
/// invisible and, for `primary_key`, actively dangerous: `#[macros(primary_key,)]` would
/// leave the field out of the key *and* put it into the generated request body — exactly
/// the key leak the body type exists to prevent, and one no round-trip test can catch,
/// because the isomorphism still holds when the partition is wrong.
fn parse_macros_attr(attr: &Attribute) -> syn::Result<MacrosDirective> {
    let unrecognized = || {
        // Report the attribute's own tokens, so the message names what was written.
        let found = match &attr.meta {
            Meta::List(list) => list.tokens.to_string(),
            other => other.to_token_stream().to_string(),
        };
        syn::Error::new_spanned(
            attr,
            format!("unrecognized directive `{found}`: {MACROS_ATTR_HELP}"),
        )
    };

    match attr.parse_args::<Meta>() {
        Ok(Meta::Path(path)) if path.is_ident("primary_key") => Ok(MacrosDirective::PrimaryKey),
        Ok(Meta::List(list))
            if list.path.is_ident("body_derive") || list.path.is_ident("update_derive") =>
        {
            let is_body = list.path.is_ident("body_derive");
            let name = if is_body {
                "body_derive"
            } else {
                "update_derive"
            };
            list.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
                .map(|paths| {
                    let paths = paths.into_iter().collect();
                    if is_body {
                        MacrosDirective::BodyDerive(paths)
                    } else {
                        MacrosDirective::UpdateDerive(paths)
                    }
                })
                .map_err(|error| {
                    syn::Error::new(
                        error.span(),
                        format!("`{name}` takes a comma-separated list of trait paths: {error}"),
                    )
                })
        }
        _ => Err(unrecognized()),
    }
}

/// Every `#[macros(...)]` directive on one item, paired with the attribute it came from so
/// a misplaced directive can be reported on the right span. Errors are combined, so a type
/// with several bad attributes reports all of them at once.
fn macros_directives(attrs: &[Attribute]) -> syn::Result<Vec<(&Attribute, MacrosDirective)>> {
    let mut directives = Vec::new();
    let mut errors: Option<syn::Error> = None;

    for attr in attrs.iter().filter(|attr| attr.path().is_ident("macros")) {
        match parse_macros_attr(attr) {
            Ok(directive) => directives.push((attr, directive)),
            Err(error) => match &mut errors {
                Some(existing) => existing.combine(error),
                None => errors = Some(error),
            },
        }
    }

    match errors {
        Some(error) => Err(error),
        None => Ok(directives),
    }
}

/// The error for a struct-level derive list written on a field. Naming the directive that
/// was actually used beats listing every directive it could have been.
fn misplaced_on_field(attr: &Attribute, directive: &str) -> syn::Error {
    syn::Error::new_spanned(
        attr,
        format!(
            "`{directive}` configures a generated type, so it belongs on the struct, not on \
             a field"
        ),
    )
}

/// Whether a field is marked `#[macros(primary_key)]`.
///
/// Anything else in a field's `#[macros(...)]` is an error: the alternative is demoting a
/// key field into the request body without a word of diagnostics.
fn is_pk_attr(attrs: &[Attribute]) -> syn::Result<bool> {
    let mut marked = false;
    for (attr, directive) in macros_directives(attrs)? {
        match directive {
            MacrosDirective::PrimaryKey => marked = true,
            MacrosDirective::BodyDerive(_) => return Err(misplaced_on_field(attr, "body_derive")),
            MacrosDirective::UpdateDerive(_) => {
                return Err(misplaced_on_field(attr, "update_derive"));
            }
        }
    }
    Ok(marked)
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

/// Reads one marked field out of `&self` by value.
///
/// `primary_key` returns the key by value from a `&self` receiver, so the field has to be
/// cloned rather than moved out. Written as a fully qualified call so it resolves to
/// `Clone::clone` regardless of what inherent `clone` the field's type might have, and so
/// clippy's `clone_on_copy` does not fire on `Copy` keys like `i64`.
fn clone_field(member: &Member) -> TokenStream2 {
    quote! { ::core::clone::Clone::clone(&self.#member) }
}

/// The name of the type a composite primary key is carried by: `{Record}PrimaryKey`,
/// alongside the `{Record}Body` and `{Record}Update` the other derives generate.
fn primary_key_ident(name: &Ident) -> Ident {
    format_ident!("{}PrimaryKey", name)
}

/// How a record's primary key is represented.
///
/// One marked field is represented by that field's own type: a lone path segment binds
/// unambiguously, so there is nothing a wrapper would fix, and `Path<i64>` keeps accepting
/// a route whatever it names the segment. Several marked fields are represented by a
/// generated `{Record}PrimaryKey` struct, because `axum::extract::Path` fills a *tuple*
/// from URI segments left to right with no name matching: a route declaring its segments in
/// a different order from the marked fields compiles, mounts, runs — and addresses the
/// wrong row, silently. A struct binds by field name, so segment order stops mattering and
/// a segment named after nothing is a `400` instead.
///
/// Built once and shared by the `PrimaryKey` and `Record` derives, so the associated type,
/// the accessor, and the pattern that takes a key apart again cannot drift.
struct PrimaryKeyShape {
    /// The generated key struct. Empty for a single-field key, which declares no new type.
    declaration: TokenStream2,
    /// What `HasPrimaryKey::PrimaryKey` is set to.
    ty: TokenStream2,
    /// The expression `primary_key` returns, read off `&self`.
    value: TokenStream2,
}

impl PrimaryKeyShape {
    /// `vis` is the record's own visibility, which the generated struct takes; each field
    /// keeps the visibility it has on the record, exactly as `{Record}Body` does.
    ///
    /// Two things both callers establish before calling: `pk_fields` is non-empty, since an
    /// unmarked struct is rejected first and an empty list would otherwise generate a key
    /// struct with no fields; and a composite key's members are named, since
    /// `try_expand_primary_key` rejects a tuple struct and `Record` only accepts
    /// named-field structs at all.
    fn new(name: &Ident, vis: &Visibility, pk_fields: &[(Member, Field)]) -> Self {
        if let [(member, field)] = pk_fields {
            let ty = &field.ty;
            return Self {
                declaration: quote! {},
                ty: quote! { #ty },
                value: clone_field(member),
            };
        }

        let key_ident = primary_key_ident(name);

        // Only the visibility and the type are carried over. The record's other attributes
        // are deliberately *not* forwarded, unlike `{Record}Body` and `{Record}Update`:
        // this type's wire format is URL path segments, not JSON, so a `rename_all` meant
        // for a request body would silently rename the path segments a route has to
        // declare, and a `deny_unknown_fields` would reject any route capturing a segment
        // the key does not name.
        let fields = pk_fields.iter().map(|(member, field)| {
            let field_vis = &field.vis;
            let ty = &field.ty;
            quote! { #field_vis #member: #ty }
        });
        let inits = pk_fields.iter().map(|(member, _)| {
            let value = clone_field(member);
            quote! { #member: #value }
        });

        // `Deserialize` is what makes this type work at all — every route trait taking the
        // key out of a path bounds it `DeserializeOwned` — so it is emitted rather than
        // asked for, unlike the `body_derive`/`update_derive` lists. Same call the
        // `str_enum!` macro makes for its fixed derive set. Every path is absolute, and
        // `serde` is reached through `sql_traits`' re-export, so the expansion needs
        // nothing in scope at the use site.
        let declaration = quote! {
            #[derive(
                ::core::clone::Clone,
                ::core::fmt::Debug,
                ::core::cmp::PartialEq,
                ::sql_traits::serde::Deserialize
            )]
            #[serde(crate = "::sql_traits::serde")]
            #vis struct #key_ident {
                #( #fields ),*
            }
        };

        Self {
            declaration,
            ty: quote! { #key_ident },
            value: quote! { #key_ident { #( #inits ),* } },
        }
    }

    /// Emits `impl ::sql_traits::HasPrimaryKey`, preceded by the key struct when there is
    /// one. Shared by the `PrimaryKey` and `Record` derives so the two cannot drift on the
    /// associated type or the accessor body.
    ///
    /// Both derives emit the declaration, and neither can double up: they both emit
    /// `impl HasPrimaryKey`, so deriving the pair together is already a duplicate-impl
    /// error.
    fn has_primary_key_impl(&self, name: &Ident) -> TokenStream2 {
        let Self {
            declaration,
            ty,
            value,
        } = self;

        quote! {
            #declaration

            impl ::sql_traits::HasPrimaryKey for #name {
                type PrimaryKey = #ty;

                fn primary_key(&self) -> <Self as ::sql_traits::HasPrimaryKey>::PrimaryKey {
                    #value
                }
            }
        }
    }

    /// The `let` binding one local per marked field, so `Record` can rebuild a record from
    /// a key. A single-field key *is* the value; a composite one is taken apart by name,
    /// which is what keeps the rebuild correct no matter what order the fields are in.
    fn destructure(&self, pk_fields: &[(Member, Field)], locals: &[Ident]) -> TokenStream2 {
        if let [local] = locals {
            return quote! { let #local = primary_key; };
        }

        let ty = &self.ty;
        let bindings = pk_fields
            .iter()
            .zip(locals)
            .map(|((member, _), local)| quote! { #member: #local });
        quote! { let #ty { #( #bindings ),* } = primary_key; }
    }
}

/// Body of the `PrimaryKey` derive.
/// - 0 fields  -> compile error.
/// - 1 field   -> PrimaryKey = <field_type>, read back as that field.
/// - N fields  -> PrimaryKey = a generated `{Name}PrimaryKey` struct with a field per marked
///   field, read back as that struct. Named-field structs only; see [`PrimaryKeyShape`] for
///   why a composite key cannot stay a tuple.
fn expand_primary_key(input: TokenStream2) -> TokenStream2 {
    try_expand_primary_key(input).unwrap_or_else(|error| error.to_compile_error())
}

fn try_expand_primary_key(input: TokenStream2) -> syn::Result<TokenStream2> {
    let input: DeriveInput = syn::parse2(input)?;

    let name = &input.ident;

    // This derive generates no body type, so a `body_derive` here is inert. Saying so
    // beats letting a directive on the wrong derive do nothing. An `update_derive` is
    // tolerated: `#[derive(PrimaryKey, Update)]` is a valid pairing.
    container_derives(&input.attrs, DeriveList::Neither)?;

    // Collect marked fields, keeping the accessor alongside the field so the associated
    // type and the `primary_key` body are built from one list and cannot fall out of order.
    let mut pk_fields: Vec<(Member, Field)> = Vec::new();

    match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(named) => {
                for field in &named.named {
                    if is_pk_attr(&field.attrs)? {
                        // Named fields always carry an ident.
                        let ident = field.ident.clone().expect("named field has an ident");
                        pk_fields.push((Member::Named(ident), field.clone()));
                    }
                }
            }
            Fields::Unnamed(unnamed) => {
                for (index, field) in unnamed.unnamed.iter().enumerate() {
                    if is_pk_attr(&field.attrs)? {
                        pk_fields.push((Member::Unnamed(Index::from(index)), field.clone()));
                    }
                }
            }
            Fields::Unit => {}
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "PrimaryKey can only be derived for structs",
            ));
        }
    }

    if pk_fields.is_empty() {
        return Err(syn::Error::new_spanned(
            &input,
            "No field marked with #[macros(primary_key)]. Mark one or more fields.",
        ));
    }

    // A tuple struct has no field names, so there is nothing for the generated key struct
    // to bind a path segment to. Leaving such a key as a tuple would keep exactly the
    // silent positional binding the struct exists to prevent, in the one place it could
    // not be fixed — so it is an error naming the shape that does work. A single marked
    // field is unaffected: it declares no key struct at all.
    if pk_fields.len() > 1
        && pk_fields
            .iter()
            .any(|(member, _)| matches!(member, Member::Unnamed(_)))
    {
        return Err(syn::Error::new_spanned(
            &input,
            "a composite primary key needs named fields: the generated key struct binds URL \
             path segments by name, and a tuple struct's fields have none",
        ));
    }

    Ok(PrimaryKeyShape::new(name, &input.vis, &pk_fields).has_primary_key_impl(name))
}

/// Reads the struct-level derive list belonging to `reader` off the container.
///
/// A derive macro cannot see sibling `#[derive(...)]` attributes — rustc strips them
/// before the macro runs — so the derives for a generated type have to be named here
/// explicitly. The *other* derive's list is skipped rather than rejected, which is what
/// lets `#[derive(Record, Update)]` carry both `body_derive` and `update_derive`.
fn container_derives(attrs: &[Attribute], reader: DeriveList) -> syn::Result<Vec<Path>> {
    let mut derives = Vec::new();
    for (attr, directive) in macros_directives(attrs)? {
        match (directive, reader) {
            (MacrosDirective::PrimaryKey, _) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`primary_key` marks a field, not the struct",
                ));
            }
            (MacrosDirective::BodyDerive(paths), DeriveList::Body)
            | (MacrosDirective::UpdateDerive(paths), DeriveList::Update) => derives.extend(paths),
            // Present for a sibling derive that does read it.
            (MacrosDirective::UpdateDerive(_), DeriveList::Body | DeriveList::Neither)
            | (MacrosDirective::BodyDerive(_), DeriveList::Update) => {}
            // `PrimaryKey` generates no body type, and cannot be derived alongside `Record`
            // — both emit `HasPrimaryKey` — so nothing on this struct will ever read it.
            (MacrosDirective::BodyDerive(_), DeriveList::Neither) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`body_derive` is only read by `#[derive(macros::Record)]`; the \
                     `PrimaryKey` derive generates no body type",
                ));
            }
        }
    }
    Ok(derives)
}

/// Whether a type is syntactically `Option<..>`.
///
/// Matches the last path segment, so `Option<T>`, `std::option::Option<T>` and
/// `::core::option::Option<T>` are all recognized. A type *alias* for `Option<T>` is not,
/// and no proc macro can resolve one. The consequence is contained: the generated field
/// still gets the correct type — one more `Option` than the record has — and only loses
/// its `deserialize_with`, so an explicit `null` reads as "leave alone" rather than
/// "clear". A wrong answer here is never a type error, only a missing capability.
fn is_option(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() {
        return false;
    }
    type_path.path.segments.last().is_some_and(|segment| {
        segment.ident == "Option"
            && matches!(
                &segment.arguments,
                PathArguments::AngleBracketed(args) if args.args.len() == 1
            )
    })
}

/// Everything except this crate's own `#[macros(...)]` attributes, which must not reach
/// the generated type. Forwarding the rest is what keeps `serde`/`ts-rs` renames aligned
/// between a record and its body — if they diverged, a generated client would disagree
/// with the wire format.
fn forwarded_attrs(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("macros"))
        .cloned()
        .collect()
}

/// Body of the `Record` derive: a superset of `PrimaryKey` that also generates the
/// key-less request-body type and the conversions between the two.
///
/// Named-field structs only — the body type is built by name, and a tuple struct would
/// need index shifting for no practical gain, since database rows are named-field structs.
fn expand_record(input: TokenStream2) -> TokenStream2 {
    try_expand_record(input).unwrap_or_else(|error| error.to_compile_error())
}

fn try_expand_record(input: TokenStream2) -> syn::Result<TokenStream2> {
    let input: DeriveInput = syn::parse2(input)?;

    let name = &input.ident;

    let Data::Struct(data_struct) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "Record can only be derived for structs",
        ));
    };
    let Fields::Named(named) = &data_struct.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "Record can only be derived for structs with named fields",
        ));
    };

    let mut pk_fields: Vec<(Member, Field)> = Vec::new();
    let mut body_fields: Vec<Field> = Vec::new();
    for field in &named.named {
        let ident = field.ident.clone().expect("named field has an ident");
        if is_pk_attr(&field.attrs)? {
            pk_fields.push((Member::Named(ident), field.clone()));
        } else {
            let mut body_field = field.clone();
            body_field.attrs = forwarded_attrs(&body_field.attrs);
            body_fields.push(body_field);
        }
    }

    if pk_fields.is_empty() {
        return Err(syn::Error::new_spanned(
            &input,
            "No field marked with #[macros(primary_key)]. Mark one or more fields.",
        ));
    }

    let body_ident = format_ident!("{}Body", name);
    let vis: &Visibility = &input.vis;
    let container_attrs = forwarded_attrs(&input.attrs);
    let derives = container_derives(&input.attrs, DeriveList::Body)?;
    let derive_attr = if derives.is_empty() {
        quote! {}
    } else {
        quote! { #[derive( #( #derives ),* )] }
    };

    // One shape, shared with the `PrimaryKey` derive: the associated type, the accessor,
    // and the pattern below that takes a key apart again are built from it together, so a
    // key field can never be rebuilt into the wrong slot.
    let shape = PrimaryKeyShape::new(name, vis, &pk_fields);
    let has_primary_key = shape.has_primary_key_impl(name);
    let key_type = &shape.ty;

    // Named locals rather than the fields' own names: a key field named `body` would
    // otherwise shadow the `body` parameter and take `body.#field` with it.
    let pk_locals: Vec<Ident> = (0..pk_fields.len())
        .map(|index| format_ident!("__pk{}", index))
        .collect();
    let destructure_key = shape.destructure(&pk_fields, &pk_locals);

    let key_inits = pk_fields
        .iter()
        .zip(&pk_locals)
        .map(|((member, _), local)| quote! { #member: #local });
    let body_idents: Vec<&Ident> = body_fields
        .iter()
        .map(|field| field.ident.as_ref().expect("named field has an ident"))
        .collect();

    Ok(quote! {
        #derive_attr
        #( #container_attrs )*
        #vis struct #body_ident {
            #( #body_fields ),*
        }

        #has_primary_key

        impl ::sql_traits::HasRequestBody for #name {
            type RequestBody = #body_ident;

            fn from_request_body(
                body: #body_ident,
                primary_key: <Self as ::sql_traits::HasPrimaryKey>::PrimaryKey,
            ) -> Self {
                #destructure_key
                Self {
                    #( #key_inits, )*
                    #( #body_idents: body.#body_idents, )*
                }
            }
        }

        // `RequestBody::Record` is bound `HasRequestBody<RequestBody = Self>`, so naming
        // the record is the whole impl: `with_key` is provided, and the bound is what
        // makes the pair unable to drift.
        impl ::sql_traits::RequestBody for #body_ident {
            type Record = #name;
        }

        impl ::core::convert::From<(#key_type, #body_ident)> for #name {
            fn from((primary_key, body): (#key_type, #body_ident)) -> Self {
                <Self as ::sql_traits::HasRequestBody>::from_request_body(body, primary_key)
            }
        }

        impl ::core::convert::From<#name> for #body_ident {
            fn from(record: #name) -> Self {
                Self {
                    #( #body_idents: record.#body_idents, )*
                }
            }
        }
    })
}

/// Body of the `Update` derive: generates the partial-update type for a record — every
/// non-key field, each of them optional — and the `HasUpdateFields`/`UpdateFields` pair
/// associating the two.
///
/// Named-field structs only, for the same reason `Record` is: the type is built by name.
fn expand_update(input: TokenStream2) -> TokenStream2 {
    try_expand_update(input).unwrap_or_else(|error| error.to_compile_error())
}

fn try_expand_update(input: TokenStream2) -> syn::Result<TokenStream2> {
    let input: DeriveInput = syn::parse2(input)?;

    let name = &input.ident;

    let Data::Struct(data_struct) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "Update can only be derived for structs",
        ));
    };
    let Fields::Named(named) = &data_struct.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "Update can only be derived for structs with named fields",
        ));
    };

    let mut has_primary_key = false;
    let mut update_fields: Vec<Field> = Vec::new();
    let mut idents: Vec<Ident> = Vec::new();

    for field in &named.named {
        // The key is never part of a partial update: it comes from the URL path, exactly as
        // it does for a request body.
        if is_pk_attr(&field.attrs)? {
            has_primary_key = true;
            continue;
        }

        let ty = &field.ty;
        let mut update_field = field.clone();
        update_field.attrs = forwarded_attrs(&field.attrs);

        // A nullable column needs three states, and only the helper keeps an explicit
        // `null` apart from an absent key. A non-nullable one needs two, which serde
        // already gives `Option<T>` for free — attaching the helper there would not even
        // type-check.
        if is_option(ty) {
            update_field.attrs.push(parse_quote! {
                #[serde(default, deserialize_with = "::sql_traits::double_option")]
            });
        }

        // One more `Option` than the record has, whatever the field was: `String` becomes
        // `Option<String>` and `Option<i32>` becomes `Option<Option<i32>>`.
        update_field.ty = parse_quote! { ::core::option::Option<#ty> };

        idents.push(field.ident.clone().expect("named field has an ident"));
        update_fields.push(update_field);
    }

    if !has_primary_key {
        return Err(syn::Error::new_spanned(
            &input,
            "No field marked with #[macros(primary_key)]. Mark one or more fields.",
        ));
    }

    let update_ident = format_ident!("{}Update", name);
    let vis: &Visibility = &input.vis;
    let container_attrs = forwarded_attrs(&input.attrs);
    let derives = container_derives(&input.attrs, DeriveList::Update)?;
    let derive_attr = if derives.is_empty() {
        quote! {}
    } else {
        quote! { #[derive( #( #derives ),* )] }
    };

    let applications = idents.iter().map(|ident| {
        quote! {
            if let ::core::option::Option::Some(value) = fields.#ident {
                record.#ident = value;
            }
        }
    });

    // A record whose only fields are its key produces an update type with no fields at all.
    // Joining zero clauses with `&&` would not compile, and the bindings would be unused.
    let (record_binding, fields_binding, is_empty_body) = if idents.is_empty() {
        (quote! { record }, quote! { _fields }, quote! { true })
    } else {
        (
            quote! { mut record },
            quote! { fields },
            quote! { #( self.#idents.is_none() )&&* },
        )
    };

    Ok(quote! {
        #derive_attr
        #( #container_attrs )*
        #vis struct #update_ident {
            #( #update_fields ),*
        }

        impl ::sql_traits::HasUpdateFields for #name {
            type UpdateFields = #update_ident;

            fn apply_update_fields(#record_binding: Self, #fields_binding: #update_ident) -> Self {
                #( #applications )*
                record
            }
        }

        // `UpdateFields::Record` is bound `HasUpdateFields<UpdateFields = Self>`, so naming
        // the record plus `is_empty` is the whole impl: `apply` is provided, and the bound
        // is what makes the pair unable to drift.
        impl ::sql_traits::UpdateFields for #update_ident {
            type Record = #name;

            fn is_empty(&self) -> bool {
                #is_empty_body
            }
        }
    })
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

/// Derive `HasPrimaryKey` by inspecting fields marked with #[macros(primary_key)].
///
/// One marked field gives `PrimaryKey = <that field's type>`. Several give a generated
/// `{Name}PrimaryKey` struct with one field per marked field, named and typed as the record
/// names and types them — so `axum::extract::Path` binds each URL segment **by name** and
/// the order a route declares them in stops mattering. A tuple would bind them
/// positionally, which compiles and runs while addressing the wrong row; a composite key on
/// a tuple struct, having no field names to bind to, is a compile error rather than a
/// silent fallback to that behaviour.
///
/// The generated struct always derives `Clone`, `Debug`, `PartialEq` and
/// `serde::Deserialize`, and takes the record's own visibility; each of its fields keeps the
/// visibility it has on the record. Its attributes are deliberately *not* copied from the
/// record: it is addressed by URL path segments rather than by JSON, so a `serde` rename
/// meant for a request body has no business renaming the segments a route must declare.
/// The name `{Name}PrimaryKey` is reserved, exactly as `{Name}Body` and `{Name}Update` are.
///
/// `primary_key` reads the marked fields back off `&self` by cloning them, so every marked
/// field's type must be `Clone`.
#[proc_macro_derive(PrimaryKey, attributes(macros))]
pub fn derive_primary_key(input: TokenStream) -> TokenStream {
    expand_primary_key(input.into()).into()
}

/// Derive `Record` — a superset of `PrimaryKey` that also generates `{Name}Body` (the
/// record's fields minus its primary key), the `HasRequestBody`/`RequestBody` pair
/// associating the two, and both `From` conversions between them.
///
/// Name the derives for the generated body type with
/// `#[macros(body_derive(Serialize, Deserialize))]`: a derive macro cannot see sibling
/// `#[derive(...)]` attributes, so they cannot be copied automatically. Every other
/// attribute on the record and its body fields — `#[serde(rename_all = "...")]` and the
/// like — is forwarded to the body automatically. Note that this includes
/// `#[serde(deny_unknown_fields)]`, which makes the generated body *reject* a payload
/// carrying the primary key rather than ignoring it.
///
/// A `#[macros(...)]` attribute that is not `primary_key` on a field or `body_derive(...)`
/// on the struct is a compile error, never a silent no-op.
///
/// A composite key gets the same generated `{Name}PrimaryKey` struct
/// `#[derive(macros::PrimaryKey)]` emits, from the same code — see that derive for what the
/// struct looks like and why it is not a tuple.
///
/// Do not derive `PrimaryKey` alongside this; both emit `impl HasPrimaryKey` (and, for a
/// composite key, both emit `{Name}PrimaryKey`), so the result is a pile of duplicate-item
/// and duplicate-impl errors.
#[proc_macro_derive(Record, attributes(macros))]
pub fn derive_record(input: TokenStream) -> TokenStream {
    expand_record(input.into()).into()
}

/// Derive `Update` — generates `{Name}Update` (every field except the primary key, each
/// wrapped in one more `Option` than the record has) plus the
/// `HasUpdateFields`/`UpdateFields` pair associating it with the record.
///
/// Name the derives for the generated type with
/// `#[macros(update_derive(Deserialize))]`, for the same reason `Record` needs
/// `body_derive`: a derive macro cannot see sibling `#[derive(...)]` attributes. Every
/// other attribute on the record and its fields is forwarded, so `serde` renames stay
/// aligned between a record and its update type.
///
/// A field whose type is syntactically `Option<..>` also gets
/// `#[serde(default, deserialize_with = "::sql_traits::double_option")]`, which is what
/// keeps an explicit `null` (clear the column) apart from an absent key (leave it alone).
/// A type *alias* for `Option<T>` cannot be recognized — no proc macro can resolve one —
/// and such a field simply loses the ability to be cleared; it is never a type error.
///
/// Designed to sit alongside `#[derive(macros::Record)]`; the two read different container
/// directives and generate different types.
#[proc_macro_derive(Update, attributes(macros))]
pub fn derive_update(input: TokenStream) -> TokenStream {
    expand_update(input.into()).into()
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

    fn expand(src: &str) -> String {
        expand_primary_key(src.parse().unwrap()).to_string()
    }

    #[test]
    fn single_marked_field_becomes_the_primary_key_type() {
        let out = expand("struct User { #[macros(primary_key)] id: i64, name: String }");
        assert!(out.contains("type PrimaryKey = i64"), "{out}");
    }

    // The whole point of the change: a composite key is a *named* struct, so
    // `axum::extract::Path` binds each URL segment by name. A tuple bound them
    // positionally, which addressed the wrong row whenever a route declared its segments in
    // a different order from the marked fields.
    #[test]
    fn multiple_marked_fields_become_a_named_struct() {
        let out = expand(
            "struct Membership { #[macros(primary_key)] user_id: i64, #[macros(primary_key)] group_id: u32 }",
        );
        assert!(
            out.contains("type PrimaryKey = MembershipPrimaryKey"),
            "{out}"
        );
        assert!(
            out.contains("struct MembershipPrimaryKey { user_id : i64 , group_id : u32 }"),
            "{out}"
        );
    }

    // `Deserialize` is what every route trait taking the key out of a path bounds on, and
    // the `crate` path is what lets a use site reach `serde` without naming it.
    #[test]
    fn the_generated_key_struct_carries_the_fixed_derive_set() {
        let out = expand(
            "struct Membership { #[macros(primary_key)] user_id: i64, #[macros(primary_key)] group_id: u32 }",
        );
        for expected in [
            ":: core :: clone :: Clone",
            ":: core :: fmt :: Debug",
            ":: core :: cmp :: PartialEq",
            ":: sql_traits :: serde :: Deserialize",
            r#"# [serde (crate = "::sql_traits::serde")]"#,
        ] {
            assert!(out.contains(expected), "missing {expected} in {out}");
        }
    }

    // A lone path segment binds unambiguously, so there is nothing a wrapper would fix —
    // and wrapping would break every `get_record(&pool, 5)` for no gain.
    #[test]
    fn a_single_marked_field_generates_no_key_struct() {
        let out = expand("struct User { #[macros(primary_key)] id: i64, name: String }");
        assert!(!out.contains("UserPrimaryKey"), "{out}");
        assert!(!out.contains("struct"), "{out}");
    }

    // The key struct is addressed by URL path segments, not by JSON. Forwarding a
    // `rename_all` meant for a request body would silently rename the segments a route has
    // to declare.
    #[test]
    fn the_record_attributes_do_not_reach_the_key_struct() {
        let out = expand(
            "#[serde(rename_all = \"camelCase\")] struct Membership { #[macros(primary_key)] user_id: i64, #[macros(primary_key)] group_id: u32 }",
        );
        assert!(!out.contains("rename_all"), "{out}");
    }

    #[test]
    fn the_key_struct_takes_the_record_visibility_and_keeps_each_field_own() {
        let out = expand(
            "pub struct Membership { #[macros(primary_key)] pub(crate) user_id: i64, #[macros(primary_key)] group_id: u32 }",
        );
        assert!(out.contains("pub struct MembershipPrimaryKey"), "{out}");
        assert!(
            out.contains("{ pub (crate) user_id : i64 , group_id : u32 }"),
            "{out}"
        );
    }

    // A tuple struct has no field names for a path segment to bind to, so there is nothing
    // to generate. Leaving it as a tuple would keep the silent positional binding in the
    // one place it could not be fixed.
    #[test]
    fn a_composite_key_on_a_tuple_struct_is_a_compile_error() {
        let out = expand("struct Pair(#[macros(primary_key)] i64, #[macros(primary_key)] u32);");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("needs named fields"), "{out}");
    }

    #[test]
    fn single_marked_field_is_read_back_by_primary_key() {
        let out = expand("struct User { #[macros(primary_key)] id: i64, name: String }");
        assert!(
            out.contains(":: core :: clone :: Clone :: clone (& self . id)"),
            "{out}"
        );
    }

    #[test]
    fn multiple_marked_fields_are_read_back_into_the_key_struct_by_name() {
        let out = expand(
            "struct Membership { #[macros(primary_key)] user_id: i64, #[macros(primary_key)] group_id: u32 }",
        );
        assert!(
            out.contains(
                "MembershipPrimaryKey { user_id : :: core :: clone :: Clone :: clone (& self . user_id) , group_id : :: core :: clone :: Clone :: clone (& self . group_id) }"
            ),
            "{out}"
        );
    }

    #[test]
    fn tuple_struct_fields_are_read_back_by_index() {
        let out = expand("struct UserId(#[macros(primary_key)] i64, String);");
        assert!(out.contains("type PrimaryKey = i64"), "{out}");
        assert!(
            out.contains(":: core :: clone :: Clone :: clone (& self . 0)"),
            "{out}"
        );
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

    fn expand_rec(src: &str) -> String {
        expand_record(src.parse().unwrap()).to_string()
    }

    #[test]
    fn record_emits_a_body_struct_without_the_key_fields() {
        let out = expand_rec(
            "pub struct Widget { #[macros(primary_key)] pub id: i64, pub name: String }",
        );
        assert!(out.contains("pub struct WidgetBody"), "{out}");
        assert!(out.contains("pub name : String"), "{out}");
        assert!(
            !out.contains("id : i64"),
            "body must not carry the key: {out}"
        );
    }

    #[test]
    fn record_applies_the_requested_body_derives() {
        let out = expand_rec(
            "#[macros(body_derive(Debug, PartialEq))] struct Widget { #[macros(primary_key)] id: i64, name: String }",
        );
        assert!(out.contains("# [derive (Debug , PartialEq)]"), "{out}");
    }

    #[test]
    fn record_forwards_non_macros_attributes_to_the_body() {
        let out = expand_rec(
            "#[serde(rename_all = \"camelCase\")] struct Widget { #[macros(primary_key)] id: i64, #[serde(rename = \"n\")] name: String }",
        );
        assert!(
            out.contains("rename_all"),
            "container attr must forward: {out}"
        );
        assert!(
            out.contains("rename = \"n\""),
            "field attr must forward: {out}"
        );
        assert!(
            !out.contains("macros"),
            "macros attrs must not forward: {out}"
        );
    }

    #[test]
    fn record_rejects_tuple_structs() {
        let out = expand_rec("struct Widget(#[macros(primary_key)] i64, String);");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("named fields"), "{out}");
    }

    #[test]
    fn record_rejects_a_struct_with_no_marked_field() {
        let out = expand_rec("struct Widget { id: i64 }");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("No field marked"), "{out}");
    }

    #[test]
    fn record_emits_both_association_impls() {
        let out = expand_rec("struct Widget { #[macros(primary_key)] id: i64, name: String }");
        assert!(
            out.contains(":: sql_traits :: HasRequestBody for Widget"),
            "{out}"
        );
        assert!(
            out.contains(":: sql_traits :: RequestBody for WidgetBody"),
            "{out}"
        );
        assert!(out.contains("type RequestBody = WidgetBody"), "{out}");
        assert!(out.contains("type Record = Widget"), "{out}");
    }

    #[test]
    fn record_emits_both_from_conversions() {
        let out = expand_rec("struct Widget { #[macros(primary_key)] id: i64, name: String }");
        assert!(
            out.contains("From < (i64 , WidgetBody) > for Widget"),
            "{out}"
        );
        assert!(out.contains("From < Widget > for WidgetBody"), "{out}");
    }

    // `#[macros(...)]` validation. Each of these was silently ignored before, and the
    // first one is the dangerous shape: a malformed key marker demoted `course_id` out of
    // the primary key and into the generated body, leaking the key into the request body.
    #[test]
    fn a_stray_comma_in_the_key_marker_is_a_compile_error() {
        let out = expand_rec(
            "struct Enrollment { #[macros(primary_key)] student_id: i64, #[macros(primary_key,)] course_id: i64, grade: String }",
        );
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("unrecognized directive"), "{out}");
        assert!(out.contains("primary_key"), "{out}");
    }

    #[test]
    fn a_stray_comma_in_the_key_marker_is_a_compile_error_for_primary_key_too() {
        let out = expand("struct User { #[macros(primary_key,)] id: i64, name: String }");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("unrecognized directive"), "{out}");
    }

    #[test]
    fn an_unrecognized_directive_name_is_a_compile_error() {
        let out = expand_rec("struct Widget { #[macros(primary_ky)] id: i64, name: String }");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("unrecognized directive `primary_ky`"), "{out}");
        assert!(out.contains("accepts exactly"), "{out}");
    }

    #[test]
    fn body_derive_in_a_non_list_form_is_a_compile_error() {
        let out = expand_rec(
            "#[macros(body_derive = \"Debug\")] struct Widget { #[macros(primary_key)] id: i64, name: String }",
        );
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("unrecognized directive"), "{out}");
        assert!(out.contains("body_derive"), "{out}");
    }

    #[test]
    fn the_plural_body_derives_typo_is_a_compile_error() {
        let out = expand_rec(
            "#[macros(body_derives(Debug))] struct Widget { #[macros(primary_key)] id: i64, name: String }",
        );
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("unrecognized directive"), "{out}");
        assert!(out.contains("body_derives"), "{out}");
    }

    #[test]
    fn a_misplaced_directive_is_a_compile_error_naming_where_it_belongs() {
        let on_a_field = expand_rec(
            "struct Widget { #[macros(primary_key)] id: i64, #[macros(body_derive(Debug))] name: String }",
        );
        assert!(on_a_field.contains("belongs on the"), "{on_a_field}");

        let on_the_struct =
            expand_rec("#[macros(primary_key)] struct Widget { #[macros(primary_key)] id: i64 }");
        assert!(
            on_the_struct.contains("marks a field, not the struct"),
            "{on_the_struct}"
        );
    }

    #[test]
    fn primary_key_derive_rejects_a_container_directive_it_cannot_act_on() {
        let out =
            expand("#[macros(body_derive(Debug))] struct User { #[macros(primary_key)] id: i64 }");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("derive(macros::Record)"), "{out}");
    }

    fn expand_upd(src: &str) -> String {
        expand_update(src.parse().unwrap()).to_string()
    }

    #[test]
    fn update_emits_a_fields_struct_without_the_key_fields() {
        let out = expand_upd(
            "pub struct Widget { #[macros(primary_key)] pub id: i64, pub name: String }",
        );
        assert!(out.contains("pub struct WidgetUpdate"), "{out}");
        assert!(
            !out.contains("id :"),
            "the key must never be part of a partial update: {out}"
        );
    }

    #[test]
    fn update_wraps_a_non_nullable_field_in_one_option() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64, name: String }");
        assert!(
            out.contains("name : :: core :: option :: Option < String >"),
            "{out}"
        );
    }

    #[test]
    fn update_wraps_a_nullable_field_in_two_options() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64, qty: Option<i32> }");
        assert!(
            out.contains("qty : :: core :: option :: Option < Option < i32 > >"),
            "{out}"
        );
    }

    // Without the helper an explicit `null` deserializes to the same `None` an absent key
    // produces, silently turning "clear this column" into "leave it alone".
    #[test]
    fn a_nullable_field_gets_the_double_option_helper_and_a_default() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64, qty: Option<i32> }");
        assert!(
            out.contains(r#"deserialize_with = "::sql_traits::double_option""#),
            "{out}"
        );
        assert!(out.contains("default"), "the helper needs `default`: {out}");
    }

    // A plain `Option<T>` field is already optional to serde, so the attribute would be
    // noise — and `deserialize_with` on it would be a type error.
    #[test]
    fn a_non_nullable_field_gets_no_serde_attribute() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64, name: String }");
        assert!(!out.contains("double_option"), "{out}");
    }

    #[test]
    fn a_fully_qualified_option_is_still_recognized_as_nullable() {
        let out = expand_upd(
            "struct Widget { #[macros(primary_key)] id: i64, qty: ::core::option::Option<i32> }",
        );
        assert!(out.contains("double_option"), "{out}");
    }

    #[test]
    fn update_emits_both_association_impls() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64, name: String }");
        assert!(
            out.contains(":: sql_traits :: HasUpdateFields for Widget"),
            "{out}"
        );
        assert!(
            out.contains(":: sql_traits :: UpdateFields for WidgetUpdate"),
            "{out}"
        );
        assert!(out.contains("type UpdateFields = WidgetUpdate"), "{out}");
        assert!(out.contains("type Record = Widget"), "{out}");
    }

    #[test]
    fn apply_writes_a_set_field_and_leaves_an_absent_one() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64, name: String }");
        assert!(
            out.contains("if let :: core :: option :: Option :: Some (value) = fields . name"),
            "{out}"
        );
        assert!(out.contains("record . name = value"), "{out}");
    }

    #[test]
    fn is_empty_checks_every_field() {
        let out = expand_upd(
            "struct Widget { #[macros(primary_key)] id: i64, name: String, qty: Option<i32> }",
        );
        assert!(
            out.contains("self . name . is_none () && self . qty . is_none ()"),
            "{out}"
        );
    }

    // A record whose only fields are its key has nothing to update, so every patch of it is
    // empty. An `is_empty` built by joining zero clauses with `&&` would not compile.
    #[test]
    fn is_empty_is_true_when_there_are_no_non_key_fields() {
        let out = expand_upd("struct Widget { #[macros(primary_key)] id: i64 }");
        assert!(
            out.contains("fn is_empty (& self) -> bool { true }"),
            "{out}"
        );
    }

    #[test]
    fn update_applies_the_requested_derives() {
        let out = expand_upd(
            "#[macros(update_derive(Debug, PartialEq))] struct Widget { #[macros(primary_key)] id: i64, name: String }",
        );
        assert!(out.contains("# [derive (Debug , PartialEq)]"), "{out}");
    }

    #[test]
    fn update_forwards_non_macros_attributes() {
        let out = expand_upd(
            "#[serde(rename_all = \"camelCase\")] struct Widget { #[macros(primary_key)] id: i64, #[serde(rename = \"n\")] name: String }",
        );
        assert!(out.contains("rename_all"), "{out}");
        assert!(out.contains("rename = \"n\""), "{out}");
        assert!(!out.contains("macros"), "{out}");
    }

    // The point of routing container directives by reader: a struct can carry both lists,
    // and each derive reads its own while stepping over the other. Before that, whichever
    // derive ran into the list it did not own rejected the whole struct.
    #[test]
    fn record_and_update_read_their_own_derive_list_and_tolerate_the_other() {
        let src = "#[macros(body_derive(Debug))] #[macros(update_derive(PartialEq))] \
                   struct Widget { #[macros(primary_key)] id: i64, name: String }";

        let record = expand_rec(src);
        assert!(!record.contains("compile_error"), "{record}");
        assert!(record.contains("# [derive (Debug)]"), "{record}");
        assert!(
            !record.contains("PartialEq"),
            "Record must not read the update list: {record}"
        );

        let update = expand_upd(src);
        assert!(!update.contains("compile_error"), "{update}");
        assert!(update.contains("# [derive (PartialEq)]"), "{update}");
        assert!(
            !update.contains("Debug"),
            "Update must not read the body list: {update}"
        );
    }

    // `#[derive(PrimaryKey, Update)]` is a valid pairing, so `PrimaryKey` has to step over
    // an `update_derive` too — while still rejecting a `body_derive`, which nothing on a
    // struct carrying `PrimaryKey` can ever read.
    #[test]
    fn primary_key_tolerates_an_update_list_but_not_a_body_list() {
        let tolerated = expand(
            "#[macros(update_derive(Debug))] struct User { #[macros(primary_key)] id: i64 }",
        );
        assert!(!tolerated.contains("compile_error"), "{tolerated}");

        let rejected =
            expand("#[macros(body_derive(Debug))] struct User { #[macros(primary_key)] id: i64 }");
        assert!(rejected.contains("compile_error"), "{rejected}");
    }

    #[test]
    fn update_rejects_tuple_structs() {
        let out = expand_upd("struct Widget(#[macros(primary_key)] i64, String);");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("named fields"), "{out}");
    }

    #[test]
    fn update_rejects_a_struct_with_no_marked_field() {
        let out = expand_upd("struct Widget { id: i64 }");
        assert!(out.contains("compile_error"), "{out}");
        assert!(out.contains("No field marked"), "{out}");
    }

    // `Record` builds its key from the same shape `PrimaryKey` does, so the struct, the
    // associated type and the pattern that takes a key apart cannot drift onto different
    // spellings — which is what would let a key field be rebuilt into the wrong slot.
    #[test]
    fn record_destructures_a_composite_key_by_name_when_rebuilding() {
        let out = expand_rec(
            "struct Membership { #[macros(primary_key)] user_id: i64, #[macros(primary_key)] group_id: u32, role: String }",
        );
        assert!(
            out.contains("struct MembershipPrimaryKey { user_id : i64 , group_id : u32 }"),
            "{out}"
        );
        assert!(
            out.contains(
                "let MembershipPrimaryKey { user_id : __pk0 , group_id : __pk1 } = primary_key"
            ),
            "{out}"
        );
        assert!(
            out.contains("From < (MembershipPrimaryKey , MembershipBody) > for Membership"),
            "{out}"
        );
    }

    // A key field named `body` must not capture the `body: MembershipBody` parameter the
    // rebuild reads its non-key fields off, which is why the destructure binds to `__pk{n}`
    // locals rather than to the fields' own names.
    #[test]
    fn a_key_field_named_body_does_not_shadow_the_request_body() {
        let out = expand_rec(
            "struct Doc { #[macros(primary_key)] body: String, #[macros(primary_key)] rev: i64, title: String }",
        );
        assert!(
            out.contains("let DocPrimaryKey { body : __pk0 , rev : __pk1 } = primary_key"),
            "{out}"
        );
        assert!(out.contains("title : body . title"), "{out}");
    }
}
