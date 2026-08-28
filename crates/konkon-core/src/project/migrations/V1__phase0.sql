CREATE TABLE source_file (
    id           INTEGER PRIMARY KEY,
    rel_path     TEXT NOT NULL UNIQUE,
    engine_id    TEXT NOT NULL,
    encoding     TEXT NOT NULL,
    sha256       TEXT NOT NULL,
    layout_json  TEXT NOT NULL
);

CREATE TABLE unit (
    id                  INTEGER PRIMARY KEY,
    file_id             INTEGER NOT NULL REFERENCES source_file(id),
    ordinal             INTEGER NOT NULL,
    kind                TEXT NOT NULL,
    jp_raw              TEXT NOT NULL,
    jp_norm             TEXT NOT NULL,
    placeholder_map_json TEXT NOT NULL,
    speaker_raw         TEXT,
    UNIQUE (file_id, ordinal)
);

CREATE TABLE translation (
    unit_id   INTEGER PRIMARY KEY REFERENCES unit(id),
    text_en   TEXT NOT NULL,
    status    TEXT NOT NULL,
    origin    TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
