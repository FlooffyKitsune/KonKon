use crate::Result;
use rusqlite::Connection;

mod embedded {
    refinery::embed_migrations!("src/project/migrations");
}

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct SourceFileRow {
    pub id: i64,
    pub rel_path: String,
    pub engine_id: String,
    pub encoding: String,
    pub sha256: String,
    pub layout_json: String,
}

#[derive(Debug, Clone)]
pub struct UnitRow {
    pub id: i64,
    pub file_id: i64,
    pub ordinal: i64,
    pub kind: String,
    pub jp_raw: String,
    pub jp_norm: String,
    pub placeholder_map_json: String,
    pub speaker_raw: Option<String>,
    pub text_en: Option<String>,
    pub status: Option<String>,
}

impl Store {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let mut conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        embedded::migrations::runner()
            .run(&mut conn)
            .map_err(|e| crate::KonkonError::Other(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn insert_source_file(
        &self,
        rel_path: &str,
        engine_id: &str,
        encoding: &str,
        sha256: &str,
        layout_json: &str,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO source_file (rel_path, engine_id, encoding, sha256, layout_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (rel_path, engine_id, encoding, sha256, layout_json),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_unit(
        &self,
        file_id: i64,
        ordinal: i64,
        kind: &str,
        jp_raw: &str,
        jp_norm: &str,
        placeholder_map_json: &str,
        speaker_raw: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO unit (file_id, ordinal, kind, jp_raw, jp_norm, placeholder_map_json, speaker_raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (file_id, ordinal, kind, jp_raw, jp_norm, placeholder_map_json, speaker_raw),
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn upsert_translation(
        &self,
        unit_id: i64,
        text_en: &str,
        status: &str,
        origin: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO translation (unit_id, text_en, status, origin, updated_at)
             VALUES (?1, ?2, ?3, ?4, datetime('now'))
             ON CONFLICT(unit_id) DO UPDATE SET
               text_en=excluded.text_en, status=excluded.status,
               origin=excluded.origin, updated_at=excluded.updated_at",
            (unit_id, text_en, status, origin),
        )?;
        Ok(())
    }

    pub fn units_for_file(&self, file_id: i64) -> Result<Vec<UnitRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.file_id, u.ordinal, u.kind, u.jp_raw, u.jp_norm,
                    u.placeholder_map_json, u.speaker_raw, t.text_en, t.status
             FROM unit u LEFT JOIN translation t ON t.unit_id = u.id
             WHERE u.file_id = ?1 ORDER BY u.ordinal",
        )?;
        let rows = stmt
            .query_map([file_id], |r| {
                Ok(UnitRow {
                    id: r.get(0)?,
                    file_id: r.get(1)?,
                    ordinal: r.get(2)?,
                    kind: r.get(3)?,
                    jp_raw: r.get(4)?,
                    jp_norm: r.get(5)?,
                    placeholder_map_json: r.get(6)?,
                    speaker_raw: r.get(7)?,
                    text_en: r.get(8)?,
                    status: r.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn all_source_files(&self) -> Result<Vec<SourceFileRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, rel_path, engine_id, encoding, sha256, layout_json FROM source_file ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SourceFileRow {
                    id: r.get(0)?,
                    rel_path: r.get(1)?,
                    engine_id: r.get(2)?,
                    encoding: r.get(3)?,
                    sha256: r.get(4)?,
                    layout_json: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
