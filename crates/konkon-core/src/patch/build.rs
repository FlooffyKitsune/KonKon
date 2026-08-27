//! Stage a patch artifact: write `patch/<rel_path>` for each file plus
//! `patch/manifest.json`.

use super::{sha256_bytes, sha256_file, Manifest, ManifestEntry};
use crate::engines::PatchFile;
use crate::project::Project;
use crate::Result;
use std::path::Path;

/// Clear any previous artifact, then stage `files` under the project's
/// `patch/` directory and record a [`Manifest`].
///
/// `orig_sha256` is computed from the current file at `game_dir/<rel_path>`,
/// or left as `""` when the target does not exist yet (a new loose file).
pub fn write_artifact(project: &Project, game_dir: &Path, files: &[PatchFile]) -> Result<Manifest> {
    let patch_dir = project.root.join("patch");
    // Clear any previous artifact, then recreate the (now empty) dir.
    if patch_dir.exists() {
        std::fs::remove_dir_all(&patch_dir)?;
    }
    std::fs::create_dir_all(&patch_dir)?;

    let mut entries = Vec::new();
    for f in files {
        let staged = patch_dir.join(&f.rel_path);
        if let Some(parent) = staged.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&staged, &f.bytes)?;

        let target = game_dir.join(&f.rel_path);
        let orig_sha256 = if target.is_file() {
            sha256_file(&target)?
        } else {
            String::new()
        };

        entries.push(ManifestEntry {
            rel_path: f.rel_path.clone(),
            orig_sha256,
            patched_sha256: sha256_bytes(&f.bytes),
        });
    }

    let manifest = Manifest { entries };
    manifest.save(&patch_dir)?;
    Ok(manifest)
}
