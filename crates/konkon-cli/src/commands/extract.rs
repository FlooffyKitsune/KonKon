//! `konkon extract` — run the engine's extractor and populate `source_file` +
//! `unit`.

use anyhow::Context;
use konkon_core::project::Project;
use std::path::Path;

pub fn run(project: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let cfg = proj.config()?;
    let (game_dir, engine) = super::resolved(&cfg)?;

    let store = proj.store()?;
    if !store.all_source_files()?.is_empty() {
        anyhow::bail!(
            "project already has extracted data — delete {} and re-run \
             `project new` to re-extract",
            project.display()
        );
    }

    let work = proj.root.join("extracted");
    let extraction = engine.extract(&game_dir, &work).context("extract")?;

    let (n_files, n_units) = store.transaction(|store| {
        let mut n_files = 0usize;
        let mut n_units = 0usize;
        for f in &extraction.files {
            let fid = store.insert_source_file(
                &f.rel_path,
                engine.id(),
                &f.encoding,
                &f.sha256,
                &f.layout_json,
            )?;
            for u in &f.units {
                store.insert_unit(
                    fid,
                    u.ordinal as i64,
                    u.kind.as_str(),
                    &u.jp_raw,
                    &u.jp_norm,
                    &u.placeholders.to_json(),
                    u.speaker_raw.as_deref(),
                )?;
                n_units += 1;
            }
            n_files += 1;
        }
        Ok((n_files, n_units))
    })?;
    println!("extracted {n_units} units from {n_files} files");
    Ok(())
}
