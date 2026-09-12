//! A sample API over the workspace's own traits.
//!
//! Every type this crate names from `axum`, `serde`, `sqlx` and `sql_traits` is reached
//! through `axum_helpers`' re-exports, exactly as the `tests/` crates do. That is not
//! stylistic: it makes building this crate an assertion that the re-export surface is
//! sufficient to write a real application, which is why the crate is a workspace member and
//! not an excluded directory.

mod authors;
mod books;
mod db;
mod reviews;

use axum_helpers::axum::{self, Router};

#[tokio::main]
async fn main() {
    let pool = db::connect_and_seed().await;

    let counts = axum_helpers::sqlx::query_as::<_, (i64, i64, i64)>(
        "SELECT (SELECT count(*) FROM author), (SELECT count(*) FROM book), (SELECT count(*) FROM review)",
    )
    .fetch_one(&pool)
    .await
    .expect("the counts");
    println!(
        "seeded {} authors, {} books, {} reviews",
        counts.0, counts.1, counts.2
    );

    let app = Router::new()
        .merge(authors::routes())
        .merge(books::routes())
        .merge(reviews::routes())
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3114")
        .await
        .expect("port 3114 to be free");

    println!("sample_api listening on http://127.0.0.1:3114");

    axum::serve(listener, app).await.expect("the server to run");
}
