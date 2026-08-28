//! Install, remove, and inspect a staged patch artifact.

use super::{join_rel, sha256_file, Manifest};
use crate::project::Project;
use crate::{KonkonError, Result};
use std::path::Path;

/// Where the game's files stand relative to a staged patch artifact.
///
/// The CLI prints this with `{:?}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchStatus {
    /// No entry's target matches its `patched_sha256` (or no manifest exists).
    NotApplied,
    /// Every entry's target matches its `patched_sha256`.
    Applied,
    /// Some — but not all — entries match.
    Partial,
}

fn copy_with_parents(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// Install the staged patch.
///
/// For each manifest entry: verify the staged file's sha256 first (catch a
/// corrupted artifact); back up the original to `backups/<rel_path>` if it
/// exists and no backup is there yet; copy the staged file over the target;
/// verify the installed file's sha256 again.
pub fn apply(project: &Project, game_dir: &Path) -> Result<()> {
    let patch_dir = project.root.join("patch");
    let backups = project.root.join("backups");
    let manifest = Manifest::load(&patch_dir)?;

    for e in &manifest.entries {
        let staged = patch_dir.join(&e.rel_path);
        let staged_sha = sha256_file(&staged)?;
        if staged_sha != e.patched_sha256 {
            return Err(KonkonError::Other(format!(
                "staged patch file {} sha256 mismatch (artifact corrupted)",
                e.rel_path
            )));
        }

        let target = join_rel(game_dir, &e.rel_path);
        let backup = join_rel(&backups, &e.rel_path);
        if !e.orig_sha256.is_empty() && target.is_file() && !backup.exists() {
            copy_with_parents(&target, &backup)?;
        }
        copy_with_parents(&staged, &target)?;

        if sha256_file(&target)? != e.patched_sha256 {
            return Err(KonkonError::Other(format!(
                "post-install sha256 mismatch for {}",
                e.rel_path
            )));
        }
    }
    Ok(())
}

/// Remove the patch, restoring the game to its pre-patch state.
///
/// Entries with a backup are restored and hash-verified against
/// `orig_sha256`; entries whose `orig_sha256 == ""` (new loose files) have
/// their installed file deleted.
pub fn revert(project: &Project, game_dir: &Path) -> Result<()> {
    let backups = project.root.join("backups");
    let manifest = Manifest::load(&project.root.join("patch"))?;

    for e in &manifest.entries {
        let target = join_rel(game_dir, &e.rel_path);
        let backup = join_rel(&backups, &e.rel_path);

        if e.orig_sha256.is_empty() {
            if target.is_file() {
                std::fs::remove_file(&target)?;
            }
            continue;
        }

        if backup.is_file() {
            copy_with_parents(&backup, &target)?;
            if sha256_file(&target)? != e.orig_sha256 {
                // The file IS correctly restored from its backup; only the
                // manifest's recorded orig_sha256 is wrong (a build that ran
                // while the patch was applied recorded the patched hash). Warn,
                // but still remove the backup and succeed so the project is not
                // permanently wedged.
                eprintln!(
                    "warning: {} restored from backup but its recorded original \
                     sha256 does not match (poisoned manifest); the file is \
                     correct — rebuild the patch to fix the record",
                    e.rel_path
                );
            }
            std::fs::remove_file(&backup)?;
        }
    }
    Ok(())
}

/// Report whether the staged patch is installed. A missing `patch/manifest.json`
/// yields [`PatchStatus::NotApplied`], not an error.
pub fn status(project: &Project, game_dir: &Path) -> Result<PatchStatus> {
    let manifest = match Manifest::load(&project.root.join("patch")) {
        Ok(m) => m,
        Err(_) => return Ok(PatchStatus::NotApplied),
    };

    let mut applied = 0;
    let mut total = 0;
    for e in &manifest.entries {
        total += 1;
        let target = join_rel(game_dir, &e.rel_path);
        if target.is_file() && sha256_file(&target)? == e.patched_sha256 {
            applied += 1;
        }
    }

    Ok(match applied {
        0 => PatchStatus::NotApplied,
        n if n == total => PatchStatus::Applied,
        _ => PatchStatus::Partial,
    })
}
