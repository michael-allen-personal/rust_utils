//! The in-memory database every run starts from.
//!
//! `sqlx` 0.9 shares one in-memory database across every connection a pool hands out, so no
//! `max_connections(1)` and no shared-cache URI is needed — a table created here is visible
//! to every connection. The same fact `axum_helpers/tests/route_responses.rs` relies on.
//!
//! Each run re-seeds from scratch, so the ids `requests.http` references are always valid.
//! Rows created through the API are gone on restart; that is the trade, since this crate
//! exists for inspecting a change rather than accumulating data.

use axum_helpers::sqlx::{self, Pool, Sqlite};

const SCHEMA: &str = "
CREATE TABLE author (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    bio  TEXT
);

CREATE TABLE book (
    id             INTEGER PRIMARY KEY,
    author_id      INTEGER NOT NULL REFERENCES author(id),
    title          TEXT NOT NULL,
    genre          TEXT NOT NULL,
    published_year INTEGER
);

CREATE TABLE review (
    book_id  INTEGER NOT NULL REFERENCES book(id),
    reviewer TEXT NOT NULL,
    rating   INTEGER NOT NULL,
    comment  TEXT,
    PRIMARY KEY (book_id, reviewer)
);
";

// Fixed ids so `requests.http` can reference them. Author 1 has a bio and author 3 does not,
// so the three-state PATCH is observable from both starting states. Twelve books is
// deliberate: the cursor route's limit policy defaults to 10, so the first page leaves a
// real second page and the `next` cursor is actually exercisable. Reviews exist only on
// books 1 and 2, so `DELETE /books/3/reviews` has a legitimate zero to report.
const SEED: &str = r#"
INSERT INTO author (id, name, bio) VALUES
    (1, 'Ursula K. Le Guin', 'Author of the Earthsea and Hainish cycles.'),
    (2, 'Terry Pratchett',   'Creator of Discworld.'),
    (3, 'Mary Roach',        NULL);

INSERT INTO book (id, author_id, title, genre, published_year) VALUES
    (1,  1, 'A Wizard of Earthsea',      'Fiction',         1968),
    (2,  1, 'The Left Hand of Darkness', 'Science Fiction', 1969),
    (3,  1, 'The Dispossessed',          'Science Fiction', 1974),
    (4,  1, 'The Tombs of Atuan',        'Fiction',         1971),
    (5,  1, 'The Lathe of Heaven',       'Science Fiction', 1971),
    (6,  2, 'Guards! Guards!',           'Fiction',         1989),
    (7,  2, 'Small Gods',                'Fiction',         1992),
    (8,  2, 'Mort',                      'Fiction',         1987),
    (9,  2, 'Going Postal',              'Fiction',         2004),
    (10, 3, 'Stiff',                     'Non-Fiction',     2003),
    (11, 3, 'Packing for Mars',          'Non-Fiction',     2010),
    (12, 3, 'Bonk',                      'Non-Fiction',     NULL);

INSERT INTO review (book_id, reviewer, rating, comment) VALUES
    (1, 'ada',   5, 'Still the best.'),
    (1, 'brian', 4, NULL),
    (1, 'cleo',  5, 'Read it twice.'),
    (2, 'ada',   5, 'A masterpiece.'),
    (2, 'brian', 3, 'Slow start.'),
    (2, 'dev',   4, NULL);
"#;

/// Builds a fresh in-memory database with the schema and seed rows already applied.
///
/// Every failure here is fatal and panics: a sample that cannot reach its database should
/// die loudly on the first line rather than serve 500s.
pub async fn connect_and_seed() -> Pool<Sqlite> {
    let pool = Pool::<Sqlite>::connect("sqlite::memory:")
        .await
        .expect("an in-memory SQLite database");

    sqlx::raw_sql(SCHEMA)
        .execute(&pool)
        .await
        .expect("the schema to apply");

    sqlx::raw_sql(SEED)
        .execute(&pool)
        .await
        .expect("the seed rows to insert");

    pool
}
