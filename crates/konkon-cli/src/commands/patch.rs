//! `konkon build` / `apply` / `revert` — stage and install the patch artifact.

use konkon_core::engines::{RepackCtx, RepackFile, RepackUnit};
use konkon_core::patch::apply as patch_apply;
use konkon_core::patch::build::write_artifact;
use konkon_core::project::Project;
use std::path::Path;

pub fn build(project: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let cfg = proj.config()?;
    let (game_dir, engine) = super::resolved(&cfg)?;
    let store = proj.store()?;

    let mut files = Vec::new();
    for sf in store.all_source_files()? {
        let mut units = Vec::new();
        for u in store.units_for_file(sf.id)? {
            // Untranslated units fall back to jp_norm, so build right after
            // extract yields a byte-identical patch.
            let en = u.text_en.clone().unwrap_or_else(|| u.jp_norm.clone());
            units.push(RepackUnit {
                ordinal: u.ordinal as usize,
                jp_norm: u.jp_norm.clone(),
                en_norm: en,
                placeholder_map_json: u.placeholder_map_json.clone(),
            });
        }
        files.push(RepackFile {
            rel_path: sf.rel_path.clone(),
            layout_json: sf.layout_json.clone(),
            units,
        });
    }

    let work = proj.root.join("extracted");
    let ctx = RepackCtx {
        game_dir: &game_dir,
        work_dir: &work,
        files,
    };
    let patch_files = engine.repack(&ctx)?;
    let manifest = write_artifact(&proj, &game_dir, &patch_files)?;
    println!("built patch: {} file(s)", manifest.entries.len());
    Ok(())
}

pub fn apply(project: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let cfg = proj.config()?;
    let (game_dir, _engine) = super::resolved(&cfg)?;
    patch_apply::apply(&proj, &game_dir)?;
    println!("{:?}", patch_apply::status(&proj, &game_dir)?);
    Ok(())
}

pub fn revert(project: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let cfg = proj.config()?;
    let (game_dir, _engine) = super::resolved(&cfg)?;
    patch_apply::revert(&proj, &game_dir)?;
    println!("{:?}", patch_apply::status(&proj, &game_dir)?);
    Ok(())
}
