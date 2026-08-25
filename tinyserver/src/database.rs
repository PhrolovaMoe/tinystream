// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{fs, io, path::PathBuf, str::FromStr};

use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

pub fn path() -> Result<PathBuf, io::Error> {
    let base = dirs::data_local_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the operating system has no local data directory",
        )
    })?;

    Ok(base.join("tinystream").join("database.sqlite3"))
}

pub async fn connect() -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let pool = connect_at(path()?).await?;
    migrate(&pool).await?;
    Ok(pool)
}

pub(crate) async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!().run(pool).await
}

async fn connect_at(path: PathBuf) -> Result<SqlitePool, Box<dyn std::error::Error>> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "database path has no parent directory",
        )
    })?;
    fs::create_dir_all(directory)?;

    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .foreign_keys(true);

    Ok(SqlitePool::connect_with(options).await?)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[tokio::test]
    async fn creates_database_and_parent_directory() {
        let root = std::env::temp_dir().join(format!(
            "tinystream-database-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("nested/database.sqlite3");

        let pool = connect_at(path.clone()).await.unwrap();
        migrate(&pool).await.unwrap();
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert!(path.is_file());
        assert_eq!(foreign_keys, 1);
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT name FROM sqlite_schema WHERE type = 'table' AND name = 'library_files'",
            )
            .fetch_one(&pool)
            .await
            .unwrap(),
            "library_files"
        );

        pool.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}
