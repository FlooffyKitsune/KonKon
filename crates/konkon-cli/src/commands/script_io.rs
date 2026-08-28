//! `konkon export` / `konkon import` — move translatable lines between the
//! project database and a JSONL file.
//!
//! One JSON object per unit:
//! `{"file":REL,"ordinal":N,"speaker":..,"jp":jp_norm,"en":text_en_or_empty}`.

use konkon_core::project::Project;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize)]
struct Line {
    file: String,
    ordinal: i64,
    #[serde(default)]
    speaker: Option<String>,
    jp: String,
    #[serde(default)]
    en: String,
}

pub fn export(project: &Path, only_file: Option<&str>, out: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let store = proj.store()?;

    let mut w = std::io::BufWriter::new(std::fs::File::create(out)?);
    let mut n = 0usize;
    for sf in store.all_source_files()? {
        if only_file.is_some_and(|f| f != sf.rel_path) {
            continue;
        }
        for u in store.units_for_file(sf.id)? {
            let line = Line {
                file: sf.rel_path.clone(),
                ordinal: u.ordinal,
                speaker: u.speaker_raw.clone(),
                jp: u.jp_norm.clone(),
                en: u.text_en.clone().unwrap_or_default(),
            };
            writeln!(w, "{}", serde_json::to_string(&line)?)?;
            n += 1;
        }
    }
    w.flush()?;
    println!("exported {n} lines to {}", out.display());
    Ok(())
}

pub fn import(project: &Path, jsonl: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let store = proj.store()?;

    // (rel_path, ordinal) -> unit id, resolved once.
    let mut unit_ids: std::collections::HashMap<(String, i64), i64> =
        std::collections::HashMap::new();
    for sf in store.all_source_files()? {
        for u in store.units_for_file(sf.id)? {
            unit_ids.insert((sf.rel_path.clone(), u.ordinal), u.id);
        }
    }

    let text = std::fs::read_to_string(jsonl)?;
    let n = store.transaction(|store| {
        let mut n = 0usize;
        for (lineno, raw) in text.lines().enumerate() {
            if raw.trim().is_empty() {
                continue;
            }
            let line: Line = serde_json::from_str(raw).map_err(|e| {
                konkon_core::KonkonError::Other(format!("{}:{}: {e}", jsonl.display(), lineno + 1))
            })?;
            if line.en.is_empty() {
                continue;
            }
            let unit_id = unit_ids
                .get(&(line.file.clone(), line.ordinal))
                .ok_or_else(|| {
                    konkon_core::KonkonError::Other(format!(
                        "unknown unit: ordinal {} in {}",
                        line.ordinal, line.file
                    ))
                })?;
            store.upsert_translation(*unit_id, &line.en, "human", "human")?;
            n += 1;
        }
        Ok(n)
    })?;
    println!("imported {n} translations");
    Ok(())
}
