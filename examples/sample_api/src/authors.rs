//! `Author` — the batteries-included path.
//!
//! One `#[derive(axum_helpers::BasicCrudRoutes)]` implements seven route traits at once, so the
//! only handwritten code here is the SQL. `GetLatestRoute` is derived separately because the
//! bundle deliberately excludes it: "the most recent row" is a domain query rather than a
//! CRUD operation, and bundling it would force every deriving type to implement
//! `GetLatestRecord`.
//!
//! `books.rs` shows the alternative shape — individual route derives and a separate
//! insert-side type — and the trade between them is the `POST /authors` note below.

use axum_helpers::async_trait::async_trait;
use axum_helpers::axum::Router;
use axum_helpers::axum::routing::{get, post};
use axum_helpers::serde;
use axum_helpers::sql_traits::{
    BulkInsertRecords, DeleteRecord, GetLatestRecord, GetRecord, InsertRecord, ListRecords,
    ReplaceRecord, UpdateFields, UpdateRecord,
};
use axum_helpers::sqlx::{self, Pool, Sqlite};
use axum_helpers::{BulkCreateRoute, CreateRoute, DeleteRoute, GetLatestRoute, GetRecordRoute};
use axum_helpers::{ListRecordsRoute, ReplaceRoute, UpdateRoute};

/// The columns of `author`, in schema order.
type AuthorRow = (i64, String, Option<String>);

fn author_from_row((id, name, bio): AuthorRow) -> Author {
    Author { id, name, bio }
}

/// `bio` is nullable on purpose: it is what makes the three-state partial update
/// observable. `AuthorUpdate::bio` comes out as `Option<Option<String>>` carrying
/// `#[serde(default, deserialize_with = "::sql_traits::double_option")]`, which the derive
/// emits because the field is syntactically `Option<..>`. Absent leaves the column alone,
/// an explicit `null` clears it.
#[derive(
    serde::Serialize,
    serde::Deserialize,
    sql_traits::Database,
    sql_traits::Record,
    sql_traits::Update,
    axum_helpers::BasicCrudRoutes,
    axum_helpers::GetLatestRoute,
)]
#[sql_traits(database = Sqlite)]
#[sql_traits(body_derive(serde::Serialize, serde::Deserialize))]
#[sql_traits(update_derive(serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
pub struct Author {
    #[sql_traits(primary_key)]
    pub id: i64,
    pub name: String,
    pub bio: Option<String>,
}

#[async_trait]
impl GetRecord for Author {
    async fn get_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, AuthorRow>("SELECT id, name, bio FROM author WHERE id = ?")
            .bind(primary_key)
            .fetch_optional(pool)
            .await
            .map(|row| row.map(author_from_row))
    }
}

#[async_trait]
impl ListRecords for Author {
    async fn list_records(pool: &Pool<Sqlite>) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, AuthorRow>("SELECT id, name, bio FROM author ORDER BY id")
            .fetch_all(pool)
            .await
            .map(|rows| rows.into_iter().map(author_from_row).collect())
    }
}

#[async_trait]
impl GetLatestRecord for Author {
    async fn get_latest_record(pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, AuthorRow>("SELECT id, name, bio FROM author ORDER BY id DESC LIMIT 1")
            .fetch_optional(pool)
            .await
            .map(|row| row.map(author_from_row))
    }
}

/// `CreateRoute` is bounded `InsertRecord + Deserialize` and `insert_record` takes `self`,
/// so the bundle's create route deserializes the **whole record** — `POST /authors` carries
/// an explicit `id`. That is what the bundle does. When a database-assigned key is wanted
/// instead, the shape is a separate insert-side type; `books.rs`'s `NewBook` is that.
#[async_trait]
impl InsertRecord for Author {
    type ReturnType = Author;

    async fn insert_record(self, pool: &Pool<Sqlite>) -> Result<Author, sqlx::Error> {
        sqlx::query_as::<_, AuthorRow>(
            "INSERT INTO author (id, name, bio) VALUES (?, ?, ?) RETURNING id, name, bio",
        )
        .bind(self.id)
        .bind(&self.name)
        .bind(&self.bio)
        .fetch_one(pool)
        .await
        .map(author_from_row)
    }
}

/// `ReturnType` is the inserted rows. `books.rs` returns a count instead, so both ends of
/// that choice appear in the sample.
#[async_trait]
impl BulkInsertRecords for Author {
    type ReturnType = Vec<Author>;

    async fn bulk_insert_records(
        pool: &Pool<Sqlite>,
        records: &[Self],
    ) -> Result<Vec<Author>, sqlx::Error> {
        // One transaction for the whole batch: without it, a failure partway through (a
        // duplicate id, say) leaves the earlier rows committed and the later ones missing,
        // which contradicts `BulkInsertRecords`'s "single operation" contract.
        let mut tx = pool.begin().await?;
        let mut inserted = Vec::with_capacity(records.len());
        for record in records {
            let row = sqlx::query_as::<_, AuthorRow>(
                "INSERT INTO author (id, name, bio) VALUES (?, ?, ?) RETURNING id, name, bio",
            )
            .bind(record.id)
            .bind(&record.name)
            .bind(&record.bio)
            .fetch_one(&mut *tx)
            .await?;
            inserted.push(author_from_row(row));
        }
        tx.commit().await?;
        Ok(inserted)
    }
}

#[async_trait]
impl ReplaceRecord for Author {
    async fn replace_record(self, pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, AuthorRow>(
            "UPDATE author SET name = ?, bio = ? WHERE id = ? RETURNING id, name, bio",
        )
        .bind(&self.name)
        .bind(&self.bio)
        .bind(self.id)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(author_from_row))
    }
}

/// Fetch-apply-replace: one of the two shapes `sql_traits` documents for a partial update,
/// and the one that keeps a small record readable. It costs a second round trip and reuses
/// `ReplaceRecord`'s statement. `books.rs` uses the other shape, `sqlx::QueryBuilder`.
///
/// The empty case needs no handling — `UpdateRoute` answers `400` before calling this,
/// because a dynamically built `UPDATE` with an empty `SET` list is a syntax error.
#[async_trait]
impl UpdateRecord for Author {
    async fn update_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
        update_fields: AuthorUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        let Some(record) = Self::get_record(pool, primary_key).await? else {
            return Ok(None);
        };
        update_fields.apply(record).replace_record(pool).await
    }
}

#[async_trait]
impl DeleteRecord for Author {
    type ReturnType = u64;

    async fn delete_record(pool: &Pool<Sqlite>, primary_key: i64) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM author WHERE id = ?")
            .bind(primary_key)
            .execute(pool)
            .await
            .map(|result| result.rows_affected())
    }
}

pub fn routes() -> Router<Pool<Sqlite>> {
    Router::new()
        .route("/authors/latest", get(Author::get_latest_route))
        .route(
            "/authors",
            get(Author::list_records_route).post(Author::create_route),
        )
        .route("/authors/bulk", post(Author::bulk_create_route))
        .route(
            "/authors/{author_id}",
            get(Author::get_record_route)
                .put(Author::replace_route)
                .patch(Author::update_route)
                .delete(Author::delete_route),
        )
}
