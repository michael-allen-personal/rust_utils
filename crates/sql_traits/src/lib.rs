// Re-exported so downstream crates can use the exact same `sqlx` version these trait
// signatures are built against. Depending on `sql_traits::sqlx` instead of a separately
// declared `sqlx` guarantees a single compiled copy, avoiding "expected `Pool`, found
// `Pool`" errors from two incompatible sqlx versions. `async_trait` is re-exported for
// convenience, since implementing these traits requires the `#[async_trait]` attribute.
// `serde` is re-exported for both reasons at once: `double_option` names its traits in a
// signature, and the types `macros::Update` generates need its derives at the use site.
pub use ::async_trait;
pub use ::serde;
pub use ::sqlx;

use ::async_trait::async_trait;
use ::sqlx::PgPool;

/// Indicates that a type has a primary key, exposing it as an associated type and reading
/// it back off a record.
pub trait HasPrimaryKey {
    type PrimaryKey: Send;

    /// Returns this record's own primary key.
    ///
    /// A composite key comes back as a generated `{Name}PrimaryKey` struct with one field
    /// per marked field, matching `PrimaryKey`. It is a struct rather than a tuple so that
    /// `axum::extract::Path` binds each URL segment by name: a tuple binds them by
    /// position, which silently addresses the wrong row whenever a route declares its
    /// segments in a different order from the marked fields. The `macros::PrimaryKey`
    /// derive implements this by cloning the marked fields, so on a derived impl those
    /// field types must be `Clone`.
    fn primary_key(&self) -> Self::PrimaryKey;
}

/// Associates a record with the request-body type carrying every field except its primary
/// key, and rebuilds the record from such a body plus the key it belongs to.
///
/// This is the record side of the pair. [`RequestBody`] is the body side; `macros::Record`
/// emits both together so the two directions cannot drift apart.
///
/// A route that replaces a record takes the key from the URL path and the body from JSON,
/// then calls [`from_request_body`](HasRequestBody::from_request_body). Because the body
/// type has no key field, there is no second key to reconcile with the path.
pub trait HasRequestBody: HasPrimaryKey + Sized {
    /// The record's fields minus its primary key.
    type RequestBody: Send;

    /// Rebuilds a full record from a key-less body and the key it belongs to.
    fn from_request_body(
        body: Self::RequestBody,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Self;
}

/// The body side of [`HasRequestBody`]: lets code holding only a body reach the record
/// type it belongs to. An inherent method would be callable only where the concrete body
/// type is known; as a trait it is reachable from generic code too.
///
/// `Record` is bound `HasRequestBody<RequestBody = Self>`, which makes the pairing
/// mutual: a body can only name a record that names it back, so the two sides cannot
/// silently drift onto different types. That same bound is what lets generic code holding
/// a body reach the record, and it makes [`with_key`](RequestBody::with_key) a provided
/// method — an implementor supplies only the associated type.
pub trait RequestBody: Sized {
    /// The record this body is missing a primary key for.
    type Record: HasRequestBody<RequestBody = Self>;

    /// Attaches a primary key, producing the full record.
    fn with_key(self, primary_key: <Self::Record as HasPrimaryKey>::PrimaryKey) -> Self::Record {
        <Self::Record as HasRequestBody>::from_request_body(self, primary_key)
    }
}

/// Associates a record with the type carrying a *partial* set of its non-key fields, and
/// merges such a set into a record.
///
/// This is the record side of the pair, mirroring [`HasRequestBody`] exactly:
/// [`UpdateFields`] is the fields side, and `macros::Update` emits both together so the
/// two directions cannot drift apart.
///
/// Where [`HasRequestBody::RequestBody`] carries *every* non-key field, an update-fields
/// type carries each of them optionally, so a `PATCH` can name only what it means to
/// change. The primary key is absent for the same reason it is absent from a request
/// body: it comes from the URL path, so there is never a second key to reconcile.
pub trait HasUpdateFields: HasPrimaryKey + Sized {
    /// The record's non-key fields, each of them optional.
    type UpdateFields: UpdateFields<Record = Self> + Send;

    /// Merges a partial set of fields into a record, returning the updated record.
    ///
    /// Every field the set leaves unspecified keeps the value it had. Consuming and
    /// returning the record rather than taking `&mut` keeps this chainable and matches
    /// the rest of these traits, none of which take a mutable reference.
    fn apply_update_fields(record: Self, fields: Self::UpdateFields) -> Self;
}

/// The fields side of [`HasUpdateFields`]: lets code holding only a partial update reach
/// the record type it belongs to, and reports whether it would change anything.
///
/// `Record` is bound `HasUpdateFields<UpdateFields = Self>`, which makes the pairing
/// mutual — a fields type can only name a record that names it back — exactly as
/// [`RequestBody`] is bound to [`HasRequestBody`]. That bound is also what makes
/// [`apply`](UpdateFields::apply) a provided method: an implementor supplies the
/// associated type and [`is_empty`](UpdateFields::is_empty), nothing more.
///
/// # Representing the three states
///
/// A partial update has to distinguish three things, and a plain `Option` only encodes
/// two. The convention these traits are built around wraps every non-key field in one
/// more `Option` than the record has:
///
/// | Record field    | Update field           | absent | `null`       | value           |
/// |-----------------|------------------------|--------|--------------|-----------------|
/// | `String`        | `Option<String>`       | `None` | *(rejected)* | `Some(v)`       |
/// | `Option<i32>`   | `Option<Option<i32>>`  | `None` | `Some(None)` | `Some(Some(v))` |
///
/// Deserializing that middle column from JSON needs [`double_option`]; a plain derive
/// collapses `null` into the absent case. `macros::Update` emits the attribute for you on
/// every field it can see is nullable.
pub trait UpdateFields: Sized {
    /// The record these fields are a partial update to.
    type Record: HasUpdateFields<UpdateFields = Self>;

    /// Whether this update would change nothing, because no field is set.
    ///
    /// `axum_helpers::UpdateRoute` answers `400` rather than sending an empty update to
    /// the database, where a dynamically built `UPDATE` with an empty `SET` list is a
    /// syntax error rather than a no-op.
    fn is_empty(&self) -> bool;

    /// Merges these fields into a record, returning the updated record.
    fn apply(self, record: Self::Record) -> Self::Record {
        <Self::Record as HasUpdateFields>::apply_update_fields(record, self)
    }
}

/// Deserializes a nullable field of an update-fields type, keeping an explicit `null`
/// distinct from an absent key.
///
/// `serde` maps a missing key and a `null` onto the same `None` for `Option<Option<T>>`,
/// which would collapse "clear this column" into "leave it alone". Reading the value as
/// `Option<T>` and wrapping it in `Some` unconditionally keeps them apart: the `Some` here
/// means "the key was present", and only `#[serde(default)]` — which `macros::Update`
/// emits alongside this — produces the `None` that means it was not.
///
/// Pair the two on every nullable field:
///
/// ```ignore
/// #[serde(default, deserialize_with = "::sql_traits::double_option")]
/// qty: Option<Option<i32>>,
/// ```
///
/// `macros::Update` emits both attributes for you. This is public because the code it
/// generates has to name it; there is little reason to call it by hand.
#[doc(hidden)]
pub fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: ::serde::Deserialize<'de>,
    D: ::serde::Deserializer<'de>,
{
    ::serde::Deserialize::deserialize(deserializer).map(Some)
}

/// Fetches the most recent record of this type from the database.
#[async_trait]
pub trait GetLatestRecord: Sized {
    async fn get_latest_record(pool: &PgPool) -> Result<Option<Self>, sqlx::Error>;
}

/// Fetches the record of this type from the database with the given primary key value.
///
/// `HasPrimaryKey::PrimaryKey` is already declared `Send`, so no extra bound is needed here.
#[async_trait]
pub trait GetRecord: Sized + HasPrimaryKey {
    async fn get_record(
        pool: &PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error>;
}

/// Fetches a single record of this type from the database matching the given filter.
#[async_trait]
pub trait GetRecordWhere<T>: Sized
where
    T: Send,
{
    async fn get_record_where(pool: &PgPool, where_params: T) -> Result<Option<Self>, sqlx::Error>;
}

/// Retrieves all records of this type from the database.
#[async_trait]
pub trait ListRecords: Sized {
    async fn list_records(pool: &PgPool) -> Result<Vec<Self>, sqlx::Error>;
}

/// Retrieves all records of this type from the database matching the given filter.
#[async_trait]
pub trait ListRecordsWhere<T>: Sized {
    async fn list_records_where(pool: &PgPool, where_params: T) -> Result<Vec<Self>, sqlx::Error>;
}

/// Inserts a single record into the database.
#[async_trait]
pub trait InsertRecord {
    /// The type returned after insertion.
    ///
    /// This is typically different from the input type because fields like
    /// the primary key are generated by the database and not provided in
    /// the creation request.
    type ReturnType: Sized;
    async fn insert_record(self, pool: &PgPool) -> Result<Self::ReturnType, sqlx::Error>;
}

/// Inserts multiple records into the database in a single operation.
#[async_trait]
pub trait BulkInsertRecords: Sized + Sync {
    /// The type returned after insertion.
    ///
    /// This is typically different from the input type because fields like
    /// the primary key are generated by the database and not provided in
    /// the creation request. For bulk operations where returning all created
    /// records may be expensive, this can also be set to `()`.
    type ReturnType: Sized;
    async fn bulk_insert_records(
        pool: &PgPool,
        records: &[Self],
    ) -> Result<Self::ReturnType, sqlx::Error>;
}

/// Replaces an entire record in the database.
///
/// The primary key is not a parameter: it travels inside `self`, and an implementation
/// reads it back with [`HasPrimaryKey::primary_key`]. On the axum side
/// `ReplaceRoute` puts it there, taking it from the URL path and assembling the record
/// from a [`HasRequestBody::RequestBody`] that has no key field at all — so there is never
/// a body key to reconcile with the path.
///
/// A key matching no row comes back as `Ok(None)`, not an error, so a route can answer
/// `404` — the same convention [`GetRecord`] follows. Returning `Result<Self, _>` would
/// force an implementation to surface a missing row as `sqlx::Error::RowNotFound`, which
/// the axum error mapping turns into a `500`.
#[async_trait]
pub trait ReplaceRecord: HasPrimaryKey + Sized
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    async fn replace_record(self, pool: &PgPool) -> Result<Option<Self>, sqlx::Error>;
}

/// Updates only the fields a partial update names, leaving every other column alone.
///
/// The fields type comes from [`HasUpdateFields`] rather than being declared here, so the
/// record/fields pair stays the single source of truth for what a partial update is, and
/// an implementation of this trait cannot pair a record with a fields type that does not
/// point back at it.
///
/// Like [`ReplaceRecord`], a key matching no row is `Ok(None)` rather than an error.
///
/// # Building the statement
///
/// An update whose `SET` list depends on which fields are present cannot be written as a
/// single `query_as!`. Two shapes work: `sqlx::QueryBuilder`, pushing a binding per field
/// that is `Some`; or a fetch-apply-replace using [`UpdateFields::apply`], which trades a
/// second round trip for keeping the compile-time-checked query macros. Either way an
/// empty update needs no statement at all — `axum_helpers::UpdateRoute` rejects one with
/// `400` before calling this, since an empty `SET` list is a syntax error.
#[async_trait]
pub trait UpdateRecord: HasUpdateFields
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    async fn update_record(
        pool: &PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
        update_fields: <Self as HasUpdateFields>::UpdateFields,
    ) -> Result<Option<Self>, sqlx::Error>;
}

/// Deletes a record from the database by its primary key.
#[async_trait]
pub trait DeleteRecord: HasPrimaryKey
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    type ReturnType: Sized;
    async fn delete_record(
        pool: &PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error>;
}

/// Deletes all records of this type from the database matching the given filter.
#[async_trait]
pub trait DeleteRecordsWhere<T>
where
    T: Send,
{
    type ReturnType: Sized;
    async fn delete_records_where(
        pool: &PgPool,
        where_params: T,
    ) -> Result<Self::ReturnType, sqlx::Error>;
}
