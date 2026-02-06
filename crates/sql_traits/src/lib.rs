use async_trait::async_trait;
use sqlx::PgExecutor;

/// Indicates that a type has a primary key, exposing it as an associated type.
pub trait HasPrimaryKey {
    type PrimaryKey: Send;
}

/// Fetches the most recent record of this type from the database.
#[async_trait]
pub trait GetLatestRecord: Sized {
    async fn get_latest_record<'e, E>(executor: E) -> Result<Option<Self>, sqlx::Error>
    where
        E: PgExecutor<'e>;
}

/// Retrieves all records of this type from the database.
#[async_trait]
pub trait ListRecords: Sized {
    async fn get_all<'e, E>(executor: E) -> Result<Vec<Self>, sqlx::Error>
    where
        E: PgExecutor<'e>;
}

/// Inserts a single record into the database.
#[async_trait]
pub trait InsertSQL {
    type ReturnType: Sized;
    async fn insert_sql<'e, E>(self, executor: E) -> Result<Self::ReturnType, sqlx::Error>
    where
        E: PgExecutor<'e>;
}

/// Inserts multiple records into the database in a single operation.
#[async_trait]
pub trait BulkInsertSQL: Sized + Sync {
    async fn bulk_insert_sql<'e, E>(executor: E, records: &[Self]) -> Result<u64, sqlx::Error>
    where
        E: PgExecutor<'e>;
}

/// Deletes a record from the database by its primary key.
#[async_trait]
pub trait DeleteSQL: HasPrimaryKey
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    type ReturnType: Sized;
    async fn delete_sql<'e, E>(
        executor: E,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error>
    where
        E: PgExecutor<'e>;
}
