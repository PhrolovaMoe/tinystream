// SPDX-License-Identifier: AGPL-3.0-or-later

use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use sqlx::{Row, SqlitePool};
use walkdir::WalkDir;

use crate::config::Library;

const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "avi", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "ts", "vob", "webm",
    "wmv",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified_ns: i64,
}

pub fn walk(library: &Library) -> impl Iterator<Item = Result<LibraryFile, io::Error>> {
    WalkDir::new(&library.path)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_file() && is_jellyfin_media(entry.path()) => {
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) => return Some(Err(error.into())),
                };
                let modified_ns = match modified_ns(&metadata) {
                    Ok(modified_ns) => modified_ns,
                    Err(error) => return Some(Err(error)),
                };
                Some(Ok(LibraryFile {
                    path: entry.into_path(),
                    size: metadata.len(),
                    modified_ns,
                }))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error.into())),
        })
}

fn modified_ns(metadata: &std::fs::Metadata) -> Result<i64, io::Error> {
    let modified = metadata.modified()?;
    let nanos = match modified.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::try_from(duration.as_nanos()).expect("u128 nanoseconds fit in i128"),
        Err(error) => {
            -i128::try_from(error.duration().as_nanos()).expect("u128 nanoseconds fit in i128")
        }
    };
    i64::try_from(nanos).map_err(|_| io::Error::other("file modification time is out of range"))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheSummary {
    pub new: usize,
    pub changed: usize,
    pub unchanged: usize,
    pub removed: usize,
}

pub async fn cache_scan(
    database: &SqlitePool,
    library: &Library,
    files: &[LibraryFile],
) -> Result<CacheSummary, Box<dyn std::error::Error>> {
    let library_path = path_text(&library.path)?;
    let cached_config = sqlx::query(
        "SELECT library_name, metadata_provider FROM library_scans WHERE library_path = ?",
    )
    .bind(&library_path)
    .fetch_optional(database)
    .await?;
    let config_changed = cached_config.is_some_and(|row| {
        row.get::<String, _>("library_name") != library.name
            || row.get::<Option<String>, _>("metadata_provider") != library.metadata_provider
    });

    let cached_rows =
        sqlx::query("SELECT path, size, modified_ns FROM library_files WHERE library_path = ?")
            .bind(&library_path)
            .fetch_all(database)
            .await?;
    let mut cached = cached_rows
        .into_iter()
        .map(|row| {
            (
                row.get::<String, _>("path"),
                (row.get::<i64, _>("size"), row.get::<i64, _>("modified_ns")),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut summary = CacheSummary::default();
    let mut current = Vec::with_capacity(files.len());
    for file in files {
        let path = path_text(&file.path)?;
        let size =
            i64::try_from(file.size).map_err(|_| io::Error::other("file size is out of range"))?;
        match cached.remove(&path) {
            None => summary.new += 1,
            Some(fingerprint) if config_changed || fingerprint != (size, file.modified_ns) => {
                summary.changed += 1;
            }
            Some(_) => summary.unchanged += 1,
        }
        current.push((path, size, file.modified_ns));
    }
    summary.removed = cached.len();

    let mut transaction = database.begin().await?;
    sqlx::query(
        "INSERT INTO library_scans (library_path, library_name, metadata_provider)
         VALUES (?, ?, ?)
         ON CONFLICT (library_path) DO UPDATE SET
             library_name = excluded.library_name,
             metadata_provider = excluded.metadata_provider",
    )
    .bind(&library_path)
    .bind(&library.name)
    .bind(&library.metadata_provider)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("DELETE FROM library_files WHERE library_path = ?")
        .bind(&library_path)
        .execute(&mut *transaction)
        .await?;
    for (path, size, modified_ns) in current {
        sqlx::query(
            "INSERT INTO library_files (library_path, path, size, modified_ns) VALUES (?, ?, ?, ?)",
        )
        .bind(&library_path)
        .bind(path)
        .bind(size)
        .bind(modified_ns)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;

    Ok(summary)
}

fn path_text(path: &Path) -> Result<String, io::Error> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"))
}

fn is_jellyfin_media(path: &std::path::Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };
    if !VIDEO_EXTENSIONS
        .iter()
        .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    {
        return false;
    }

    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };

    has_episode_marker(stem) || has_movie_name(path, stem)
}

fn has_episode_marker(stem: &str) -> bool {
    stem.as_bytes().windows(6).any(|marker| {
        marker[0] == b'S'
            && marker[1].is_ascii_digit()
            && marker[2].is_ascii_digit()
            && marker[3] == b'E'
            && marker[4].is_ascii_digit()
            && marker[5].is_ascii_digit()
    })
}

fn has_movie_name(path: &std::path::Path, stem: &str) -> bool {
    let Some(parent_name) = path
        .parent()
        .and_then(std::path::Path::file_name)
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    movie_stem_matches(stem, parent_name)
        || strip_movie_qualifiers(parent_name).is_some_and(|title| movie_stem_matches(stem, title))
}

fn movie_stem_matches(stem: &str, title: &str) -> bool {
    stem == title
        || stem
            .strip_prefix(title)
            .is_some_and(|suffix| suffix.starts_with(" - "))
}

fn strip_movie_qualifiers(folder: &str) -> Option<&str> {
    let mut title = folder.trim_end();
    let original = title;

    while title.ends_with(']') {
        let start = title.rfind(" [")?;
        title = title[..start].trim_end();
    }

    if title.ends_with(')')
        && let Some(start) = title.rfind(" (")
    {
        let year = &title[start + 2..title.len() - 1];
        if year.len() == 4 && year.bytes().all(|digit| digit.is_ascii_digit()) {
            title = title[..start].trim_end();
        }
    }

    (title != original && !title.is_empty()).then_some(title)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    async fn memory_database() -> SqlitePool {
        let database = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        crate::database::migrate(&database).await.unwrap();
        database
    }

    #[test]
    fn walks_regular_files_recursively() {
        let root = std::env::temp_dir().join(format!(
            "tinystream-library-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("season")).unwrap();
        fs::write(root.join("ignored.mkv"), []).unwrap();
        fs::write(root.join("season/Show S01E01.mkv"), []).unwrap();
        fs::write(root.join("season/Show s01e02.mkv"), []).unwrap();
        fs::create_dir(root.join("Movie (2026)")).unwrap();
        fs::write(root.join("Movie (2026)/Movie (2026) - 1080p.mp4"), []).unwrap();
        fs::write(root.join("Movie (2026)/Movie.mp4"), []).unwrap();
        fs::write(root.join("Movie (2026)/Unrelated.mp4"), []).unwrap();
        fs::write(root.join("Movie (2026)/poster.jpg"), []).unwrap();

        let library = Library {
            name: "Videos".into(),
            path: root.clone(),
            metadata_provider: None,
        };
        let mut files = walk(&library)
            .map(|entry| entry.unwrap().path.strip_prefix(&root).unwrap().to_owned())
            .collect::<Vec<_>>();
        files.sort();

        assert_eq!(
            files,
            [
                PathBuf::from("Movie (2026)/Movie (2026) - 1080p.mp4"),
                PathBuf::from("Movie (2026)/Movie.mp4"),
                PathBuf::from("season/Show S01E01.mkv")
            ]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn cache_detects_file_and_library_changes() {
        let database = memory_database().await;
        let mut library = Library {
            name: "Videos".into(),
            path: PathBuf::from("/media/videos"),
            metadata_provider: None,
        };
        let first = LibraryFile {
            path: library.path.join("Movie/Movie.mkv"),
            size: 100,
            modified_ns: 1,
        };
        let second = LibraryFile {
            path: library.path.join("Show/Show S01E01.mkv"),
            size: 200,
            modified_ns: 2,
        };

        assert_eq!(
            cache_scan(&database, &library, &[first.clone(), second.clone()])
                .await
                .unwrap(),
            CacheSummary {
                new: 2,
                ..CacheSummary::default()
            }
        );
        assert_eq!(
            cache_scan(&database, &library, &[first.clone(), second.clone()])
                .await
                .unwrap(),
            CacheSummary {
                unchanged: 2,
                ..CacheSummary::default()
            }
        );

        let changed = LibraryFile {
            size: 101,
            modified_ns: 3,
            ..first.clone()
        };
        assert_eq!(
            cache_scan(&database, &library, std::slice::from_ref(&changed))
                .await
                .unwrap(),
            CacheSummary {
                changed: 1,
                removed: 1,
                ..CacheSummary::default()
            }
        );

        library.metadata_provider = Some("different-provider".into());
        assert_eq!(
            cache_scan(&database, &library, &[changed]).await.unwrap(),
            CacheSummary {
                changed: 1,
                ..CacheSummary::default()
            }
        );
    }
}
