CREATE TABLE library_scans (
    library_path TEXT PRIMARY KEY NOT NULL,
    library_name TEXT NOT NULL,
    metadata_provider TEXT
) STRICT;

CREATE TABLE library_files (
    library_path TEXT NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    PRIMARY KEY (library_path, path),
    FOREIGN KEY (library_path) REFERENCES library_scans (library_path)
        ON DELETE CASCADE
) STRICT;
