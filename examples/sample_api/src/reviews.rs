//! `Review` — the composite primary key.
//!
//! Two fields marked `#[sql_traits(primary_key)]` generate `ReviewPrimaryKey { book_id,
//! reviewer }` — a **named struct, not a tuple**, so `axum::extract::Path` binds each URL
//! segment by name. A tuple would bind by position, which compiles, mounts, runs, and
//! addresses the wrong row whenever a route declares its segments in a different order from
//! the marked fields.
//!
//! The consequence is visible in `routes()` below: the segments in
//! `/books/{book_id}/reviews/{reviewer}` must be *named* after the key's fields. Their
//! declared order does not matter, extra segments are ignored, and a key field no segment
//! names is a `400` from the extractor before the handler runs.
//!
//! `sql_traits::PrimaryKey` is deliberately **not** derived here: `Record` is a superset of it,
//! and deriving both would emit `impl HasPrimaryKey` and `ReviewPrimaryKey` twice.
//!
//! `/books/{book_id}/reviews/page` is mounted before `/books/{book_id}/reviews/{reviewer}`
//! and, as with `books.rs`'s `/books/page`, axum prefers the static segment — so a reviewer
//! literally named `page` is unreachable through the singular-record route. That is the
//! documented cost of the static-before-dynamic layout, not a bug to fix here.

use axum_helpers::async_trait::async_trait;
use axum_helpers::axum::Router;
use axum_helpers::axum::routing::{delete, get, post};
use axum_helpers::serde;
use axum_helpers::sql_traits::{
    DeleteRecord, DeleteRecordsWhere, GetRecord, InsertRecord, ListRecordsWherePaginated,
    OffsetPagination, OffsetParams, Page, ReplaceRecord, UpdateFields, UpdateRecord,
};
use axum_helpers::sqlx::{self, Pool, Sqlite};
use axum_helpers::{
    CreateRoute, DefaultOffsetParamsQuery, DeleteRecordsWhereRoute, DeleteRoute, GetRecordRoute,
    ListRecordsWherePaginatedRoute, ReplaceRoute, UpdateRoute,
};

/// The columns of `review`, in schema order.
type ReviewRow = (i64, String, i64, Option<String>);

fn review_from_row((book_id, reviewer, rating, comment): ReviewRow) -> Review {
    Review {
        book_id,
        reviewer,
        rating,
        comment,
    }
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    sql_traits::Database,
    sql_traits::Record,
    sql_traits::Update,
    axum_helpers::GetRecordRoute,
    axum_helpers::ReplaceRoute,
    axum_helpers::UpdateRoute,
    axum_helpers::DeleteRoute,
    axum_helpers::CreateRoute,
)]
#[sql_traits(database = Sqlite)]
#[sql_traits(body_derive(serde::Serialize, serde::Deserialize))]
#[sql_traits(update_derive(serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
pub struct Review {
    #[sql_traits(primary_key)]
    pub book_id: i64,
    #[sql_traits(primary_key)]
    pub reviewer: String,
    pub rating: i64,
    pub comment: Option<String>,
}

#[async_trait]
impl GetRecord for Review {
    async fn get_record(
        pool: &Pool<Sqlite>,
        primary_key: ReviewPrimaryKey,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, ReviewRow>(
            "SELECT book_id, reviewer, rating, comment FROM review \
             WHERE book_id = ? AND reviewer = ?",
        )
        .bind(primary_key.book_id)
        .bind(&primary_key.reviewer)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(review_from_row))
    }
}

/// A composite-key record on the create side: the whole record is the body, key included,
/// because there is no database-assigned column to leave out.
#[async_trait]
impl InsertRecord for Review {
    type ReturnType = Review;

    async fn insert_record(self, pool: &Pool<Sqlite>) -> Result<Review, sqlx::Error> {
        sqlx::query_as::<_, ReviewRow>(
            "INSERT INTO review (book_id, reviewer, rating, comment) VALUES (?, ?, ?, ?) \
             RETURNING book_id, reviewer, rating, comment",
        )
        .bind(self.book_id)
        .bind(&self.reviewer)
        .bind(self.rating)
        .bind(&self.comment)
        .fetch_one(pool)
        .await
        .map(review_from_row)
    }
}

#[async_trait]
impl ReplaceRecord for Review {
    async fn replace_record(self, pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, ReviewRow>(
            "UPDATE review SET rating = ?, comment = ? WHERE book_id = ? AND reviewer = ? \
             RETURNING book_id, reviewer, rating, comment",
        )
        .bind(self.rating)
        .bind(&self.comment)
        .bind(self.book_id)
        .bind(&self.reviewer)
        .fetch_optional(pool)
        .await
        .map(|row| row.map(review_from_row))
    }
}

#[async_trait]
impl UpdateRecord for Review {
    async fn update_record(
        pool: &Pool<Sqlite>,
        primary_key: ReviewPrimaryKey,
        update_fields: ReviewUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        let Some(record) = Self::get_record(pool, primary_key).await? else {
            return Ok(None);
        };
        update_fields.apply(record).replace_record(pool).await
    }
}

#[async_trait]
impl DeleteRecord for Review {
    type ReturnType = u64;

    async fn delete_record(
        pool: &Pool<Sqlite>,
        primary_key: ReviewPrimaryKey,
    ) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM review WHERE book_id = ? AND reviewer = ?")
            .bind(primary_key.book_id)
            .bind(&primary_key.reviewer)
            .execute(pool)
            .await
            .map(|result| result.rows_affected())
    }
}

/// The filter behind every route scoped to one book's reviews. Its own `PathParams`, via
/// the blanket `From<T> for T`.
#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
pub struct ReviewFilter {
    pub book_id: i64,
}

/// `DeleteRecordsWhereRoute` **serializes** its return value, unlike `DeleteRoute`, which
/// discards it — so a caller can see what the delete actually matched. That is why this
/// `ReturnType` is a count and the one on `DeleteRecord` above may as well be anything.
#[async_trait]
impl DeleteRecordsWhere<ReviewFilter> for Review {
    type ReturnType = u64;

    async fn delete_records_where(
        pool: &Pool<Sqlite>,
        where_params: ReviewFilter,
    ) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM review WHERE book_id = ?")
            .bind(where_params.book_id)
            .execute(pool)
            .await
            .map(|result| result.rows_affected())
    }
}

impl DeleteRecordsWhereRoute<ReviewFilter> for Review {
    type PathParams = ReviewFilter;
}

/// Filtered pagination: the filter narrows what is counted as well as what is returned.
/// Everything `ListRecordsPaginated` documents applies — the opt-in total especially.
#[async_trait]
impl ListRecordsWherePaginated<ReviewFilter, OffsetParams> for Review {
    async fn list_records_where_paginated(
        pool: &Pool<Sqlite>,
        where_params: ReviewFilter,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        let rows = sqlx::query_as::<_, ReviewRow>(
            "SELECT book_id, reviewer, rating, comment FROM review \
             WHERE book_id = ? ORDER BY reviewer LIMIT ? OFFSET ?",
        )
        .bind(where_params.book_id)
        .bind(i64::from(params.limit))
        .bind(i64::from(params.offset))
        .fetch_all(pool)
        .await?;

        let total = if params.include_total {
            let (count,) =
                sqlx::query_as::<_, (i64,)>("SELECT count(*) FROM review WHERE book_id = ?")
                    .bind(where_params.book_id)
                    .fetch_one(pool)
                    .await?;
            Some(count as u32)
        } else {
            None
        };

        Ok(Page {
            data: rows.into_iter().map(review_from_row).collect(),
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total,
            },
        })
    }
}

impl ListRecordsWherePaginatedRoute<ReviewFilter, DefaultOffsetParamsQuery> for Review {
    type PathParams = ReviewFilter;
}

pub fn routes() -> Router<Pool<Sqlite>> {
    Router::new()
        .route("/reviews", post(Review::create_route))
        .route(
            "/books/{book_id}/reviews/page",
            get(<Review as ListRecordsWherePaginatedRoute<
                ReviewFilter,
                DefaultOffsetParamsQuery,
            >>::list_records_where_paginated_route),
        )
        .route(
            "/books/{book_id}/reviews",
            delete(<Review as DeleteRecordsWhereRoute<ReviewFilter>>::delete_records_where_route),
        )
        // The segments are *named* after `ReviewPrimaryKey`'s fields. That is the whole
        // reason the generated key is a struct: rename either segment and this stops
        // binding, with a `400` from the extractor rather than a wrong row.
        .route(
            "/books/{book_id}/reviews/{reviewer}",
            get(Review::get_record_route)
                .put(Review::replace_route)
                .patch(Review::update_route)
                .delete(Review::delete_route),
        )
}
