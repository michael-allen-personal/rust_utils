//! Consumer-perspective compile test for the route derive macros.
//!
//! This is a separate crate whose ONLY dependencies are `axum_helpers` and `macros`.
//! It deliberately does not depend on `serde`, `sqlx`, or `sql_traits` by name — every
//! such type is reached through `axum_helpers`' re-exports. If the derive macros emitted
//! bare `serde::`/`sql_traits::` paths, this crate would fail to compile, so the fact
//! that it builds is the assertion that the generated output is self-contained.
//!
//! Note: the derived `impl` blocks are type-checked whether or not they are ever used,
//! so their mere existence forces every generated path and trait bound to resolve.

use axum_helpers::async_trait::async_trait;
use axum_helpers::sqlx::{self, PgPool};
use axum_helpers::{
    BulkCreateRoute, CreateRoute, DeleteRoute, GetRecordRoute, ListRecordsRoute, ReplaceRoute,
    UpdateRoute,
};

// serde is reached via the re-export; `#[serde(crate = ...)]` points the derive's
// generated code at the same re-exported path.
//
// `BasicCrudRoutes` covers every CRUD operation, so this fixture carries `Record` and
// `Update` alongside it: those supply the `HasPrimaryKey`, `HasRequestBody` and
// `HasUpdateFields` impls that the bundled write routes take as supertraits. A non-key
// field is what makes the generated body and update types non-empty.
#[derive(
    axum_helpers::serde::Serialize,
    axum_helpers::serde::Deserialize,
    macros::Record,
    macros::Update,
    macros::BasicCrudRoutes,
)]
#[macros(body_derive(axum_helpers::serde::Deserialize))]
#[macros(update_derive(axum_helpers::serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
struct Widget {
    #[macros(primary_key)]
    id: i64,
    name: String,
}

// Supertrait impls the route derives require. Bodies only need to type-check. Note there is
// no `GetLatestRecord` impl here: `BasicCrudRoutes` does not bundle `GetLatestRoute`, so if
// it started emitting that impl again this file would stop compiling on the missing supertrait.

#[async_trait]
impl axum_helpers::sql_traits::GetRecord for Widget {
    async fn get_record(
        _pool: &PgPool,
        _primary_key: <Self as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl axum_helpers::sql_traits::ListRecords for Widget {
    async fn list_records(_pool: &PgPool) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::InsertRecord for Widget {
    type ReturnType = ();
    async fn insert_record(self, _pool: &PgPool) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::BulkInsertRecords for Widget {
    type ReturnType = ();
    async fn bulk_insert_records(
        _pool: &PgPool,
        _records: &[Self],
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::DeleteRecord for Widget {
    type ReturnType = ();
    async fn delete_record(
        _pool: &PgPool,
        _primary_key: <Self as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl axum_helpers::sql_traits::ReplaceRecord for Widget {
    async fn replace_record(self, _pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        Ok(Some(self))
    }
}

#[async_trait]
impl axum_helpers::sql_traits::UpdateRecord for Widget {
    async fn update_record(
        _pool: &PgPool,
        primary_key: <Self as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey,
        update_fields: <Self as axum_helpers::sql_traits::HasUpdateFields>::UpdateFields,
    ) -> Result<Option<Self>, sqlx::Error> {
        use axum_helpers::sql_traits::UpdateFields as _;
        Ok(Some(update_fields.apply(Widget {
            id: primary_key,
            name: "widget".to_string(),
        })))
    }
}

// `GetLatestRoute` is a standalone derive rather than part of `BasicCrudRoutes`, so it gets
// its own type: `Gizmo` carries only what that one derive needs.
#[derive(axum_helpers::serde::Serialize, macros::GetLatestRoute)]
#[serde(crate = "axum_helpers::serde")]
struct Gizmo {
    #[allow(dead_code)]
    id: i64,
}

#[async_trait]
impl axum_helpers::sql_traits::GetLatestRecord for Gizmo {
    async fn get_latest_record(_pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

// Concrete check for the route traits without lifetime parameters. The `CreateRoute`
// and `BulkCreateRoute` impls are verified by the compiler through the derive above.
// The `DeserializeOwned` bound is restated because a trait's `where` clause is not
// elaborated into a generic caller's environment. Concrete impls (what the derive emits)
// do not need this — they get the requirement checked at the impl site.
fn assert_routes<T>()
where
    T: axum_helpers::GetRecordRoute
        + axum_helpers::ListRecordsRoute
        + axum_helpers::ReplaceRoute
        + axum_helpers::UpdateRoute
        + axum_helpers::DeleteRoute,
    <T as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey:
        axum_helpers::serde::de::DeserializeOwned,
    <T as axum_helpers::sql_traits::HasRequestBody>::RequestBody:
        axum_helpers::serde::de::DeserializeOwned + Send + 'static,
    <T as axum_helpers::sql_traits::HasUpdateFields>::UpdateFields:
        axum_helpers::serde::de::DeserializeOwned + Send + 'static,
{
}

fn assert_get_latest_route<T: axum_helpers::GetLatestRoute>() {}

#[test]
fn basic_crud_routes_derive_is_self_contained() {
    assert_routes::<Widget>();
}

/// Every handler the bundle emits, on one router. `assert_routes` proves the impls exist
/// with their bounds satisfied; mounting is what proves axum will actually accept them, and
/// it is the check that matches how the derive is used.
fn mount_basic_crud() -> axum_helpers::axum::Router<PgPool> {
    use axum_helpers::axum::routing::{delete, get, patch, post, put};

    axum_helpers::axum::Router::<PgPool>::new()
        .route("/widgets", post(Widget::create_route))
        .route("/widgets/bulk", post(Widget::bulk_create_route))
        .route("/widgets", get(Widget::list_records_route))
        .route("/widgets/{id}", get(Widget::get_record_route))
        .route("/widgets/{id}", put(Widget::replace_route))
        .route("/widgets/{id}", patch(Widget::update_route))
        .route("/widgets/{id}", delete(Widget::delete_route))
}

#[test]
fn every_bundled_crud_handler_mounts() {
    let _ = mount_basic_crud();
}

/// `GetLatestRoute` is no longer in the `BasicCrudRoutes` bundle, so the standalone derive
/// is the only thing emitting `::axum_helpers::GetLatestRoute` — checked here on its own type.
#[test]
fn get_latest_route_derive_is_self_contained() {
    assert_get_latest_route::<Gizmo>();
}

// `Record` generates the key-less request body and its `HasRequestBody`/`RequestBody`
// association; `ReplaceRoute` is the standalone derive built on top of it. `Sprocket` gets
// its own type (rather than reusing `Gizmo` above) for the same reason every other fixture
// in this file does: one derive combination per type keeps each compile assertion isolated.
#[derive(
    axum_helpers::serde::Serialize,
    axum_helpers::serde::Deserialize,
    macros::Record,
    macros::ReplaceRoute,
)]
#[macros(body_derive(axum_helpers::serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
struct Sprocket {
    #[macros(primary_key)]
    id: i64,
    label: String,
}

#[async_trait]
impl axum_helpers::sql_traits::ReplaceRecord for Sprocket {
    async fn replace_record(self, _pool: &PgPool) -> Result<Option<Self>, sqlx::Error> {
        Ok(Some(self))
    }
}

/// Mounting is where axum's `Handler` bounds are actually enforced.
fn mount_replace() -> axum_helpers::axum::Router<PgPool> {
    axum_helpers::axum::Router::<PgPool>::new().route(
        "/sprockets/{id}",
        axum_helpers::axum::routing::put(Sprocket::replace_route),
    )
}

#[test]
fn replace_route_derive_mounts() {
    let _ = mount_replace();
}

/// The design's headline tolerance claim, which nothing else on this branch exercises: a
/// client that sends the primary key in the JSON body anyway still succeeds, because serde
/// ignores unknown fields by default. The key is discarded — the body type has no field to
/// put it in — and the record's key comes from the path instead.
///
/// This holds only while the record does not carry `#[serde(deny_unknown_fields)]`, which
/// forwards to the generated body and turns the extra `id` into a 422. `Sprocket` does not
/// carry it, which is the case the claim is about.
#[test]
fn a_body_carrying_the_primary_key_still_deserializes_with_the_key_discarded() {
    use axum_helpers::sql_traits::RequestBody;

    let body: SprocketBody = axum_helpers::serde_json::from_str(r#"{"id": 5, "label": "cog"}"#)
        .expect("unknown fields are ignored");
    assert_eq!(body.label, "cog");

    // The key that ends up on the record is the path's, not the body's.
    let record = body.with_key(7);
    assert_eq!(record.id, 7);
    assert_eq!(record.label, "cog");
}

// `Record` and `Update` are designed to be derived together: they read different container
// directives and generate different types, and only `Record` emits `HasPrimaryKey`. This is
// the one fixture carrying both, so it is what proves they compose — and that the update
// type's generated `::sql_traits::double_option` path resolves from a crate that reaches
// everything else through `axum_helpers`' re-exports.
#[derive(axum_helpers::serde::Serialize, macros::Record, macros::Update, macros::UpdateRoute)]
#[macros(body_derive(axum_helpers::serde::Deserialize))]
#[macros(update_derive(axum_helpers::serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
struct Cog {
    #[macros(primary_key)]
    id: i64,
    label: String,
    /// Nullable, so the generated update field is doubly wrapped and carries the helper.
    weight: Option<i32>,
}

#[async_trait]
impl axum_helpers::sql_traits::UpdateRecord for Cog {
    async fn update_record(
        _pool: &PgPool,
        primary_key: <Self as axum_helpers::sql_traits::HasPrimaryKey>::PrimaryKey,
        update_fields: <Self as axum_helpers::sql_traits::HasUpdateFields>::UpdateFields,
    ) -> Result<Option<Self>, sqlx::Error> {
        use axum_helpers::sql_traits::UpdateFields as _;
        Ok(Some(update_fields.apply(Cog {
            id: primary_key,
            label: "cog".to_string(),
            weight: None,
        })))
    }
}

fn mount_update() -> axum_helpers::axum::Router<PgPool> {
    axum_helpers::axum::Router::<PgPool>::new().route(
        "/cogs/{id}",
        axum_helpers::axum::routing::patch(Cog::update_route),
    )
}

#[test]
fn update_route_derive_mounts() {
    let _ = mount_update();
}

/// The derived update type has to behave the same as the hand-written one: absent leaves a
/// column alone, an explicit null clears it. Only the generated `deserialize_with` keeps
/// those apart, and this is the assertion that it was actually emitted.
#[test]
fn the_derived_update_type_distinguishes_an_absent_field_from_an_explicit_null() {
    use axum_helpers::sql_traits::UpdateFields as _;

    let cog = || Cog {
        id: 1,
        label: "cog".to_string(),
        weight: Some(5),
    };

    let absent: CogUpdate =
        axum_helpers::serde_json::from_str("{}").expect("an empty patch is valid");
    assert!(absent.is_empty());
    assert_eq!(absent.apply(cog()).weight, Some(5));

    let cleared: CogUpdate =
        axum_helpers::serde_json::from_str(r#"{"weight": null}"#).expect("null is valid");
    assert!(!cleared.is_empty(), "an explicit null is a change");
    assert_eq!(cleared.apply(cog()).weight, None);
}
