//! The API against PostgreSQL. Runs when PAPERLOOM_TEST_DATABASE_URL names
//! an empty database (CI starts one); otherwise says so and passes.

#[path = "api.rs"]
#[allow(dead_code)]
mod api;

use paperloom_server::db::{Database, Postgres};

#[tokio::test]
async fn syncs_a_library_in_postgres() {
    let Ok(url) = std::env::var("PAPERLOOM_TEST_DATABASE_URL") else {
        eprintln!("PAPERLOOM_TEST_DATABASE_URL is not set: skipped");
        return;
    };
    let db = Postgres::connect(&url).await.expect("connect to the test database");
    api::syncs_a_library(Database::Postgres(db)).await;
}
