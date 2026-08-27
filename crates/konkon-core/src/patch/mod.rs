//! Reversible on-disk patch artifact: build / apply / revert / verify-hashes.
//!
//! [`build::write_artifact`] stages patched files under `patch/` plus a
//! [`Manifest`] of original/patched sha256. [`apply::apply`] backs up each
//! original to `backups/` before overwriting it; [`apply::revert`] restores
//! from backup and hash-verifies the result.

pub mod apply;
pub mod build;

use serde::{Deserialize, Serialize};

/// One patched file: where it goes and the hashes that bracket the change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Path relative to the game directory, using `/` separators.
    pub rel_path: String,
    /// sha256 of the pre-existing original, or `""` if it was a new loose file.
    pub orig_sha256: String,
    /// sha256 of the staged patched bytes.
    pub patched_sha256: String,
}

/// The full set of files a patch artifact installs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// Load `<patch_dir>/manifest.json`.
    pub fn load(patch_dir: &std::path::Path) -> crate::Result<Self> {
        let text = std::fs::read_to_string(patch_dir.join("manifest.json"))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// Write `<patch_dir>/manifest.json` (pretty JSON).
    pub fn save(&self, patch_dir: &std::path::Path) -> crate::Result<()> {
        std::fs::write(
            patch_dir.join("manifest.json"),
            serde_json::to_string_pretty(self)?,
        )?;
        Ok(())
    }
}

/// Lowercase-hex sha256 of a file's contents.
pub(crate) fn sha256_file(path: &std::path::Path) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Lowercase-hex sha256 of a byte slice.
pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
