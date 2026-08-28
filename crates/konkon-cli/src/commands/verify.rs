//! `konkon verify` — prove the extract/rebuild round-trip is lossless.
//!
//! For each `source_file`: read its archive member, DSC-decompress it if needed,
//! and compare an identity rebuild (no replacements) of the recorded
//! [`ScriptLayout`] against those decompressed bytes. The comparison is at the
//! **decompressed** level — `repack` stores script members RAW.

use anyhow::anyhow;
use konkon_core::engines::ethornell::archive::{read_arc, Member};
use konkon_core::engines::ethornell::layout::ScriptLayout;
use konkon_core::engines::ethornell::{dsc, script};
use konkon_core::project::Project;
use std::collections::BTreeMap;
use std::path::Path;

pub fn run(project: &Path) -> anyhow::Result<()> {
    let proj = Project::open(project)?;
    let cfg = proj.config()?;
    let (game_dir, _engine) = super::resolved(&cfg)?;
    let store = proj.store()?;

    // Cache each archive's members once (keyed by arc stem).
    let mut arc_cache: BTreeMap<String, Vec<Member>> = BTreeMap::new();
    let mut mismatches = 0usize;

    for sf in store.all_source_files()? {
        let (stem, member_name) = sf
            .rel_path
            .split_once('/')
            .ok_or_else(|| anyhow!("malformed rel_path {}", sf.rel_path))?;

        if !arc_cache.contains_key(stem) {
            let arc_path = game_dir.join(format!("{stem}.arc"));
            let bytes = std::fs::read(&arc_path)
                .map_err(|e| anyhow!("read {}: {e}", arc_path.display()))?;
            arc_cache.insert(stem.to_string(), read_arc(&bytes)?);
        }
        let member = arc_cache[stem]
            .iter()
            .find(|m| m.name == member_name)
            .ok_or_else(|| anyhow!("member {member_name} not in {stem}.arc"))?;

        let decompressed = if dsc::is_dsc(&member.data) {
            dsc::decompress(&member.data)?
        } else {
            member.data.clone()
        };

        let layout = ScriptLayout::from_json(&sf.layout_json)?;
        let rebuilt = script::rebuild(&layout, &BTreeMap::new())?;

        if rebuilt == decompressed {
            println!("OK {}", sf.rel_path);
        } else {
            println!(
                "MISMATCH {} (rebuilt {} bytes vs original {} bytes)",
                sf.rel_path,
                rebuilt.len(),
                decompressed.len()
            );
            mismatches += 1;
        }
    }

    if mismatches > 0 {
        anyhow::bail!("{mismatches} file(s) failed the round-trip");
    }
    println!("all files round-trip byte-identical");
    Ok(())
}
