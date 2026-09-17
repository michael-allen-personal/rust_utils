//! Every pool-taking trait is generic over the database, asserted from outside the crate.
//!
//! This crate reaches `sqlx` only through `sql_traits`' re-export, so it also proves the
//! re-export surface is enough to implement every one of the thirteen pool-taking traits
//! against a non-Postgres driver. Nothing here connects: the fixtures are compile
//! assertions, and `tests/pagination.rs` is where real queries run.

#![allow(dead_code)]

use sql_traits::async_trait::async_trait;
use sql_traits::sqlx::{self, Pool, Sqlite};
use sql_traits::{
    BulkInsertRecords, CursorPagination, CursorParams, DeleteRecord, DeleteRecordsWhere,
    GetLatestRecord, GetRecord, GetRecordWhere, HasDatabase, HasPrimaryKey, InsertRecord,
    ListRecords, ListRecordsPaginated, ListRecordsWhere, ListRecordsWherePaginated,
    OffsetPagination, OffsetParams, Page, ReplaceRecord, UpdateRecord,
};

#[derive(sql_traits::Database, sql_traits::Update)]
#[sql_traits(database = Sqlite)]
struct Widget {
    #[sql_traits(primary_key)]
    id: i64,
    name: String,
}

/// A minimal filter for the `*Where` traits, which this file otherwise has no equivalent
/// for. Its field is never read — these are compile assertions, not behaviour tests.
struct WidgetFilter {
    name: String,
}

impl HasPrimaryKey for Widget {
    type PrimaryKey = i64;
    fn primary_key(&self) -> i64 {
        self.id
    }
}

#[async_trait]
impl GetLatestRecord for Widget {
    async fn get_latest_record(_pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl GetRecord for Widget {
    async fn get_record(
        _pool: &Pool<Sqlite>,
        _primary_key: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl ListRecords for Widget {
    async fn list_records(_pool: &Pool<Sqlite>) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl InsertRecord for Widget {
    type ReturnType = ();
    async fn insert_record(self, _pool: &Pool<Sqlite>) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl DeleteRecord for Widget {
    type ReturnType = ();
    async fn delete_record(_pool: &Pool<Sqlite>, _primary_key: i64) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl GetRecordWhere<WidgetFilter> for Widget {
    async fn get_record_where(
        _pool: &Pool<Sqlite>,
        _where_params: WidgetFilter,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl ListRecordsWhere<WidgetFilter> for Widget {
    async fn list_records_where(
        _pool: &Pool<Sqlite>,
        _where_params: WidgetFilter,
    ) -> Result<Vec<Self>, sqlx::Error> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl BulkInsertRecords for Widget {
    type ReturnType = ();
    async fn bulk_insert_records(
        _pool: &Pool<Sqlite>,
        _records: &[Self],
    ) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl ReplaceRecord for Widget {
    async fn replace_record(self, _pool: &Pool<Sqlite>) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

/// `WidgetUpdate` is `sql_traits::Update`'s output on `Widget` above — the pairing that would
/// otherwise take a hand-written `HasUpdateFields`/`UpdateFields` impl to get here.
#[async_trait]
impl UpdateRecord for Widget {
    async fn update_record(
        _pool: &Pool<Sqlite>,
        _primary_key: i64,
        _update_fields: WidgetUpdate,
    ) -> Result<Option<Self>, sqlx::Error> {
        Ok(None)
    }
}

#[async_trait]
impl DeleteRecordsWhere<WidgetFilter> for Widget {
    type ReturnType = ();
    async fn delete_records_where(
        _pool: &Pool<Sqlite>,
        _where_params: WidgetFilter,
    ) -> Result<(), sqlx::Error> {
        Ok(())
    }
}

#[async_trait]
impl ListRecordsPaginated<OffsetParams> for Widget {
    async fn list_records_paginated(
        _pool: &Pool<Sqlite>,
        params: OffsetParams,
    ) -> Result<Page<Self, OffsetPagination>, sqlx::Error> {
        Ok(Page {
            data: Vec::new(),
            pagination: OffsetPagination {
                offset: params.offset,
                limit: params.limit,
                total: params.include_total.then_some(0),
            },
        })
    }
}

/// Cursor mode this time, so the file exercises both pagination shapes rather than offset
/// mode twice. The cursor is `i64`, a row id, which is the case that "costs nothing" per
/// the crate's own docs on [`sql_traits::CursorPagination`].
#[async_trait]
impl ListRecordsWherePaginated<WidgetFilter, CursorParams<i64>> for Widget {
    async fn list_records_where_paginated(
        _pool: &Pool<Sqlite>,
        _where_params: WidgetFilter,
        params: CursorParams<i64>,
    ) -> Result<Page<Self, CursorPagination<i64>>, sqlx::Error> {
        Ok(Page {
            data: Vec::new(),
            pagination: CursorPagination {
                limit: params.limit,
                next: None,
            },
        })
    }
}

/// The associated type is what the pool parameter follows, so reading it back here pins
/// the association the rest of the file relies on.
#[test]
fn the_record_names_its_database() {
    fn assert_sqlite<T: HasDatabase<Database = Sqlite>>() {}
    assert_sqlite::<Widget>();
}
