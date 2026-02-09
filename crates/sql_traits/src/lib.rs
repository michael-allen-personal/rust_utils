use async_trait::async_trait;
use sqlx::PgPool;

/// Indicates that a type has a primary key, exposing it as an associated type.
pub trait HasPrimaryKey {
    type PrimaryKey: Send;
}

/// Fetches the most recent record of this type from the database.
#[async_trait]
pub trait GetLatestRecord: Sized {
    async fn get_latest_record(pool: &PgPool) -> Result<Option<Self>, sqlx::Error>;
}

/// Retrieves all records of this type from the database.
#[async_trait]
pub trait ListRecords: Sized {
    async fn get_all(pool: &PgPool) -> Result<Vec<Self>, sqlx::Error>;
}

/// Inserts a single record into the database.
#[async_trait]
pub trait InsertSQL {
    type ReturnType: Sized;
    async fn insert_sql(self, pool: &PgPool) -> Result<Self::ReturnType, sqlx::Error>;
}

/// Inserts multiple records into the database in a single operation.
#[async_trait]
pub trait BulkInsertSQL: Sized + Sync {
    async fn bulk_insert_sql(pool: &PgPool, records: &[Self]) -> Result<u64, sqlx::Error>;
}

/// Deletes a record from the database by its primary key.
#[async_trait]
pub trait DeleteSQL: HasPrimaryKey
where
    <Self as HasPrimaryKey>::PrimaryKey: Send,
{
    type ReturnType: Sized;
    async fn delete_sql(
        pool: &PgPool,
        primary_key: <Self as HasPrimaryKey>::PrimaryKey,
    ) -> Result<Self::ReturnType, sqlx::Error>;
}
