//! `Book` — the a-la-carte path.
//!
//! Individual route derives instead of `BasicCrudRoutes`, because the create side is a
//! separate type: `NewBook` has no `id`, so the database assigns it. That is the case
//! `sql_traits::Database`'s own documentation describes — an insert-side type needing
//! `HasDatabase` without the body-type generation — and the contrast with `authors.rs`,
//! where the bundle's `POST` carries an explicit id, is the point of having both.
//!
//! Pagination and the `*Where` family are further down this file.

use axum_helpers::async_trait::async_trait;
use axum_helpers::axum::Router;
use axum_helpers::axum::routing::{get, post};
use axum_helpers::serde;
use axum_helpers::sql_traits::{
    BulkInsertRecords, CursorPagination, CursorParams, DeleteRecord, GetRecord, GetRecordWhere,
    InsertRecord, ListRecords, ListRecordsPaginated, ListRecordsWhere, OffsetPagination,
    OffsetParams, Page, ReplaceRecord, UpdateRecord,
};
use axum_helpers::sqlx::{self, Pool, QueryBuilder, Sqlite};
use axum_helpers::{
    BulkCreateRoute, CreateRoute, CursorParamsQuery, DefaultOffsetParamsQuery, DeleteRoute,
    GetRecordRoute, GetRecordWhereRoute, ListRecordsPaginatedRoute, ListRecordsRoute,
    ListRecordsWhereRoute, ReplaceRoute, UpdateRoute,
};
use generic_helpers::str_enum;

// The per-variant `#[serde(rename = "...")]` is required, not decoration: a plain derive
// would serialize `NonFiction` while `as_str` says `"Non-Fiction"`, and the wire format
// would drift from the display string.
//
// `#[serde(try_from = "String")]` is the documented integration point, not a derived
// `Deserialize`: `crates/generic_helpers/CLAUDE.md` notes the `TryFrom` impls exist
// specifically "because `FromStr` is unreachable from `#[serde(try_from = "String")]`
// and other `TryFrom`-bounded positions", and both route through the same private
// `accepts` helper as `FromStr`, so alias/case/separator handling cannot diverge from
// the `str_enum!`-generated parse entry points. A plain derived `Deserialize` would
// instead read only `#[serde(rename = "...")]`, accept exactly the three canonical
// strings, and reject `"Nonfiction"`/`"sci-fi"`/`"SciFi"`.
// `ParsingError`'s `#[display(...)]` from `error_set!` satisfies the `Display` bound
// `#[serde(try_from = "...")]` requires on the conversion error.
//
// With `try_from` in play, the per-variant `#[serde(rename = "...")]` attributes no
// longer affect deserialization — `try_from` bypasses the generated `Deserialize` body
// entirely — but they still keep `Serialize` aligned with `as_str()`.
str_enum! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(crate = "axum_helpers::serde", try_from = "String")]
    pub enum Genre {
        Fiction => "Fiction",
        #[serde(rename = "Non-Fiction")]
        NonFiction => "Non-Fiction" | "Nonfiction",
        #[serde(rename = "Science Fiction")]
        ScienceFiction => "Science Fiction" | "SciFi",
    }
}

/// The columns of `book`, in schema order. `genre` arrives as TEXT and becomes a `Genre`
/// in `book_from_row`.
type BookRow = (i64, i64, String, String, Option<i32>);

/// `Genre` moves between Rust and SQLite as TEXT — written with `as_str()`, read back
/// through the `TryFrom<String>` that `str_enum!` generates. No `sqlx::Type`/`Decode` impl
/// is needed, and a value the column should never hold surfaces as a decode error rather
/// than silently becoming a default.
///
/// `Genre::try_from`'s error is `generic_helpers::errors::ParsingError`, an `error_set!`
/// type — it implements `std::error::Error + Send + Sync + 'static`, so it satisfies
/// `sqlx::Error::Decode`'s `Box<dyn Error + Send + Sync>` bound directly (verified: this
/// builds). No `.map_err(.. sqlx::Error::Protocol ..)` fallback is needed here.
fn book_from_row(
    (id, author_id, title, genre, published_year): BookRow,
) -> Result<Book, sqlx::Error> {
    Ok(Book {
        id,
        author_id,
        title,
        genre: Genre::try_from(genre).map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
        published_year,
    })
}

fn books_from_rows(rows: Vec<BookRow>) -> Result<Vec<Book>, sqlx::Error> {
    rows.into_iter().map(book_from_row).collect()
}

#[derive(
    serde::Serialize,
    serde::Deserialize,
    sql_traits::Database,
    sql_traits::Record,
    sql_traits::Update,
    axum_helpers::GetRecordRoute,
    axum_helpers::ListRecordsRoute,
    axum_helpers::ReplaceRoute,
    axum_helpers::UpdateRoute,
    axum_helpers::DeleteRoute,
)]
#[sql_traits(database = Sqlite)]
#[sql_traits(body_derive(serde::Serialize, serde::Deserialize))]
#[sql_traits(update_derive(serde::Deserialize))]
#[serde(crate = "axum_helpers::serde")]
pub struct Book {
    #[sql_traits(primary_key)]
    pub id: i64,
    pub author_id: i64,
    pub title: String,
    pub genre: Genre,
    pub published_year: Option<i32>,
}

/// The insert side. No `id` field, so the database assigns one — which is why `Book` cannot
/// derive `BasicCrudRoutes`: the bundle's `CreateRoute` deserializes the record itself.
/// `sql_traits::Database` exists standalone for exactly this shape.
#[derive(
    serde::Deserialize,
    sql_traits::Database,
    axum_helpers::CreateRoute,
    axum_helpers::BulkCreateRoute,
)]
#[sql_traits(database = Sqlite)]
#[serde(crate = "axum_helpers::serde")]
pub struct NewBook {
    pub author_id: i64,
    pub title: String,
    pub genre: Genre,
    pub published_year: Option<i32>,
}

#[async_trait]
impl InsertRecord for NewBook {
    type ReturnType = Book;

    async fn insert_record(self, pool: &Pool<Sqlite>) -> Result<Book, sqlx::Error> {
        let row = sqlx::query_as::<_, BookRow>(
            "INSERT INTO book (author_id, title, genre, published_year) VALUES (?, ?, ?, ?) \
             RETURNING id, author_id, title, genre, published_year",
        )
        .bind(self.author_id)
        .bind(&self.title)
        .bind(self.genre.as_str())
        .bind(self.published_year)
        .fetch_one(pool)
        .await?;
        book_from_row(row)
    }
}

/// A count rather than the rows. `authors.rs` returns the inserted records, so both ends of
/// the `ReturnType` choice appear in the sample.
#[async_trait]
impl BulkInsertRecords for NewBook {
    type ReturnType = u64;

    async fn bulk_insert_records(
        pool: &Pool<Sqlite>,
        records: &[Self],
    ) -> Result<u64, sqlx::Error> {
        // One transaction for the whole batch: without it, a failure partway through (a
        // duplicate id, say) leaves the earlier rows committed and the later ones missing,
        // which contradicts `BulkInsertRecords`'s "single operation" contract.
        let mut tx = pool.begin().await?;
        let mut inserted = 0;
        for record in records {
            inserted += sqlx::query(
                "INSERT INTO book (author_id, title, genre, published_year) VALUES (?, ?, ?, ?)",
            )
            .bind(record.author_id)
            .bind(&record.title)
            .bind(record.genre.as_str())
            .bind(record.published_year)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        }
        tx.commit().await?;
        Ok(inserted)
    }
}

#[async_trait]
impl GetRecord for Book {
    async fn get_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query_as::<_, BookRow>(
            "SELECT id, author_id, title, genre, published_year FROM book WHERE id = ?",
        )
        .bind(primary_key)
        .fetch_optional(pool)
        .await?;
        row.map(book_from_row).transpose()
    }
}

#[async_trait]
impl ListRecords for Book {
    async fn list_records(pool: &Pool<Sqlite>) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BookRow>(
            "SELECT id, author_id, title, genre, published_year FROM book ORDER BY id",
        )
        .fetch_all(pool)
        .await?;
        books_from_rows(rows)
    }
}

#[async_trait]
impl ReplaceRecord for Book {
    async fn replace_record(self, pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query_as::<_, BookRow>(
            "UPDATE book SET author_id = ?, title = ?, genre = ?, published_year = ? \
             WHERE id = ? RETURNING id, author_id, title, genre, published_year",
        )
        .bind(self.author_id)
        .bind(&self.title)
        .bind(self.genre.as_str())
        .bind(self.published_year)
        .bind(self.id)
        .fetch_optional(pool)
        .await?;
        row.map(book_from_row).transpose()
    }
}

/// `sqlx::QueryBuilder`, pushing one binding per field that is `Some` — the other shape
/// `sql_traits` documents for a partial update, and the one that matters once a record has
/// several columns, since it touches the database once instead of twice.
///
/// `published_year` is the three-state field: `Some(None)` writes NULL, `None` leaves the
/// column out of the `SET` list entirely. `genre` is `Option<Genre>`, not
/// `Option<Option<Genre>>` — `Genre` is not syntactically `Option<..>`, so the `Update`
/// derive gives it no `double_option` treatment. That asymmetry between the two optional
/// columns is correct: a book's genre has no "clear it" state, only "leave it" or "set it".
///
/// The empty case needs no handling — `UpdateRoute` answers `400` before calling this.
#[async_trait]
impl UpdateRecord for Book {
    async fn update_record(
        pool: &Pool<Sqlite>,
        primary_key: i64,
        update_fields: BookUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        let mut builder = QueryBuilder::<Sqlite>::new("UPDATE book SET ");
        let mut separated = builder.separated(", ");

        if let Some(author_id) = update_fields.author_id {
            separated
                .push("author_id = ")
                .push_bind_unseparated(author_id);
        }
        if let Some(title) = update_fields.title {
            separated.push("title = ").push_bind_unseparated(title);
        }
        if let Some(genre) = update_fields.genre {
            separated
                .push("genre = ")
                .push_bind_unseparated(genre.as_str());
        }
        if let Some(published_year) = update_fields.published_year {
            separated
                .push("published_year = ")
                .push_bind_unseparated(published_year);
        }

        builder
            .push(" WHERE id = ")
            .push_bind(primary_key)
            .push(" RETURNING id, author_id, title, genre, published_year");

        let row = builder
            .build_query_as::<BookRow>()
            .fetch_optional(pool)
            .await?;
        row.map(book_from_row).transpose()
    }
}

#[async_trait]
impl DeleteRecord for Book {
    type ReturnType = u64;

    async fn delete_record(pool: &Pool<Sqlite>, primary_key: i64) -> Result<u64, sqlx::Error> {
        sqlx::query("DELETE FROM book WHERE id = ?")
            .bind(primary_key)
            .execute(pool)
            .await
            .map(|result| result.rows_affected())
    }
}

// --- the *Where family ------------------------------------------------------------------

/// The filter behind `/authors/{author_id}/books`. It serves as its own `PathParams`
/// through the blanket `From<T> for T`, which is why `type PathParams = BookFilter` below
/// type-checks with no conversion written.
///
/// The `*Where` traits have no derive: `PathParams` has to be chosen by the implementor and
/// cannot be inferred from the record, which is exactly why these four impls are written out.
#[derive(serde::Deserialize)]
#[serde(crate = "axum_helpers::serde")]
pub struct BookFilter {
    pub author_id: i64,
}

#[async_trait]
impl ListRecordsWhere<BookFilter> for Book {
    async fn list_records_where(
        pool: &Pool<Sqlite>,
        where_params: BookFilter,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BookRow>(
            "SELECT id, author_id, title, genre, published_year FROM book \
             WHERE author_id = ? ORDER BY id",
        )
        .bind(where_params.author_id)
        .fetch_all(pool)
        .await?;
        books_from_rows(rows)
    }
}

impl ListRecordsWhereRoute<BookFilter> for Book {
    type PathParams = BookFilter;
}

#[async_trait]
impl GetRecordWhere<BookFilter> for Book {
    async fn get_record_where(
        pool: &Pool<Sqlite>,
        where_params: BookFilter,
    ) -> Result<Option<Self>, sqlx::Error> {
        let row = sqlx::query_as::<_, BookRow>(
            "SELECT id, author_id, title, genre, published_year FROM book \
             WHERE author_id = ? ORDER BY id LIMIT 1",
        )
        .bind(where_params.author_id)
        .fetch_optional(pool)
        .await?;
        row.map(book_from_row).transpose()
    }
}

impl GetRecordWhereRoute<BookFilter> for Book {
    type PathParams = BookFilter;
}

// --- pagination, both modes -------------------------------------------------------------

/// Offset mode.
///
/// `total` is filled **if and only if** `include_total` was asked for. The compiler cannot
/// enforce an "if and only if", so it is a contract, and a caller who did not ask must not
/// be charged for the `count(*) OVER ()` over the whole set.
///
/// Nothing is clamped: `params.limit` arrived already checked against the route's maximum,
/// and a silently reduced page is indistinguishable from a short last page. There is
/// likewise no ceiling on `offset` — a very deep one scans and discards, which is the
/// problem cursor mode exists to solve.
#[async_trait]
impl ListRecordsPaginated<OffsetParams> for Book {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        let rows = sqlx::query_as::<_, BookRow>(
            "SELECT id, author_id, title, genre, published_year FROM book \
             ORDER BY id LIMIT ? OFFSET ?",
        )
        .bind(i64::from(params.limit))
        .bind(i64::from(params.offset))
        .fetch_all(pool)
        .await?;

        let total = if params.include_total {
            let (count,) = sqlx::query_as::<_, (i64,)>("SELECT count(*) FROM book")
                .fetch_one(pool)
                .await?;
            Some(count as u32)
        } else {
            None
        };

        Ok(Page {
            data: books_from_rows(rows)?,
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total,
            },
        })
    }
}

/// Cursor mode, over the row id.
///
/// `id` is unique, so ordering by it alone is already a total order — no tiebreaker is
/// needed here. A sort on something non-unique (`published_year`, say) would have to end
/// with a unique column and encode the whole sort key in the cursor, or pages would both
/// skip and duplicate rows.
///
/// `next` comes from fetching `limit + 1` rows and dropping the extra: anything else either
/// guesses or pays for a second count.
#[async_trait]
impl ListRecordsPaginated<CursorParams<i64>> for Book {
    async fn list_records_paginated(
        pool: &Pool<Sqlite>,
        params: CursorParams<i64>,
    ) -> Result<Page<Self, CursorPagination<i64>>, sqlx::Error> {
        let limit = usize::from(params.limit);

        let mut rows = sqlx::query_as::<_, BookRow>(
            "SELECT id, author_id, title, genre, published_year FROM book \
             WHERE id > ? ORDER BY id LIMIT ?",
        )
        .bind(params.cursor.unwrap_or(0))
        .bind(i64::from(params.limit) + 1)
        .fetch_all(pool)
        .await?;

        // The extra row only answers "is there another page"; it is not part of this one.
        let next = if rows.len() > limit {
            rows.truncate(limit);
            rows.last().map(|row| row.0)
        } else {
            None
        };

        Ok(Page {
            data: books_from_rows(rows)?,
            pagination: CursorPagination {
                limit: params.limit,
                next,
            },
        })
    }
}

/// Two impls of one provided method, which is what forces the turbofish at the mount site.
impl ListRecordsPaginatedRoute<DefaultOffsetParamsQuery> for Book {}

/// A non-default policy — 10 per page, 50 at most — so both const parameters are written
/// out. They have no defaults on purpose: `CursorParamsQuery<i64, 10>` would read as "cap
/// this route at 10" and would silently mean a default of 10 with the inherited maximum of
/// 200. A transposed pair fails to build, but only under a real `cargo build`; `cargo check`
/// never evaluates the constant.
impl ListRecordsPaginatedRoute<CursorParamsQuery<i64, 10, 50>> for Book {}

pub fn routes() -> Router<Pool<Sqlite>> {
    Router::new()
        .route(
            "/books",
            get(Book::list_records_route).post(NewBook::create_route),
        )
        .route("/books/bulk", post(NewBook::bulk_create_route))
        // Static segments alongside `/books/{book_id}`. axum prefers a static segment over
        // a parameter, so these do not conflict — but a static segment added carelessly
        // here would shadow a legitimate key value.
        .route(
            "/books/page",
            get(
                <Book as ListRecordsPaginatedRoute<DefaultOffsetParamsQuery>>::list_records_paginated_route,
            ),
        )
        .route(
            "/books/cursor",
            get(
                <Book as ListRecordsPaginatedRoute<CursorParamsQuery<i64, 10, 50>>>::list_records_paginated_route,
            ),
        )
        .route(
            "/books/{book_id}",
            get(Book::get_record_route)
                .put(Book::replace_route)
                .patch(Book::update_route)
                .delete(Book::delete_route),
        )
        .route(
            "/authors/{author_id}/books",
            get(<Book as ListRecordsWhereRoute<BookFilter>>::list_records_where_route),
        )
        .route(
            "/authors/{author_id}/books/first",
            get(<Book as GetRecordWhereRoute<BookFilter>>::get_record_where_route),
        )
}
