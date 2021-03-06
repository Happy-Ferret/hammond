//! Database Setup. This is only public to help with some unit tests.
// Diesel embed_migrations! triggers the lint
#![allow(unused_imports)]

use diesel::prelude::*;
use diesel::r2d2;
use diesel::r2d2::ConnectionManager;

use std::io;
use std::path::PathBuf;

use errors::DataError;

#[cfg(not(test))]
use xdg_dirs;

type Pool = r2d2::Pool<ConnectionManager<SqliteConnection>>;

embed_migrations!("migrations/");

lazy_static! {
    static ref POOL: Pool = init_pool(DB_PATH.to_str().unwrap());
}

#[cfg(not(test))]
lazy_static! {
    static ref DB_PATH: PathBuf = xdg_dirs::PODCASTS_XDG
        .place_data_file("podcasts.db")
        .unwrap();
}

#[cfg(test)]
extern crate tempdir;

#[cfg(test)]
lazy_static! {
    static ref TEMPDIR: tempdir::TempDir = { tempdir::TempDir::new("podcasts_unit_test").unwrap() };
    static ref DB_PATH: PathBuf = TEMPDIR.path().join("podcasts.db");
}

/// Get an r2d2 `SqliteConnection`.
pub(crate) fn connection() -> Pool {
    POOL.clone()
}

fn init_pool(db_path: &str) -> Pool {
    let manager = ConnectionManager::<SqliteConnection>::new(db_path);
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .build(manager)
        .expect("Failed to create pool.");

    {
        let db = pool.get().expect("Failed to initialize pool.");
        run_migration_on(&*db).expect("Failed to run migrations during init.");
    }
    info!("Database pool initialized.");
    pool
}

fn run_migration_on(connection: &SqliteConnection) -> Result<(), DataError> {
    info!("Running DB Migrations...");
    // embedded_migrations::run(connection)?;
    embedded_migrations::run_with_output(connection, &mut io::stdout()).map_err(From::from)
}

/// Reset the database into a clean state.
// Test share a Temp file db.
#[cfg(test)]
pub fn truncate_db() -> Result<(), DataError> {
    let db = connection();
    let con = db.get()?;
    con.execute("DELETE FROM episodes")?;
    con.execute("DELETE FROM shows")?;
    con.execute("DELETE FROM source")?;
    Ok(())
}
