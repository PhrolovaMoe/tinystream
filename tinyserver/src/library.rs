// SPDX-License-Identifier: AGPL-3.0-or-later

use std::path::PathBuf;

use walkdir::WalkDir;

use crate::config::Library;

const VIDEO_EXTENSIONS: &[&str] = &[
    "3gp", "avi", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "mts", "ts", "vob", "webm",
    "wmv",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryFile {
    pub path: PathBuf,
}

pub fn walk(library: &Library) -> impl Iterator<Item = Result<LibraryFile, walkdir::Error>> {
    WalkDir::new(&library.path)
        .into_iter()
        .filter_map(|entry| match entry {
            Ok(entry) if entry.file_type().is_file() && is_jellyfin_media(entry.path()) => {
                Some(Ok(LibraryFile {
                    path: entry.into_path(),
                }))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
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
}
