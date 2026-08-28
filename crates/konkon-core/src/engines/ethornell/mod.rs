//! Ethornell / BGI engine: probe, DSC-aware extract, raw-store repack.
//!
//! Wires together the format primitives:
//! - [`archive`] — `BURIKO ARC20` container read/build (Task 6)
//! - [`dsc`] — `DSC FORMAT 1.00` decompressor (Task 6B)
//! - [`script`] / [`layout`] — `BurikoCompiledScriptVer1.00` parse/rebuild (Tasks 7/8)
//! - [`crate::script::placeholder`] — control-code normalization (Task 4)

pub mod archive;
mod disasm;
pub mod dsc;
pub mod layout;
pub mod script;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use encoding_rs::SHIFT_JIS;
use sha2::{Digest, Sha256};

use super::{EmitKind, Engine, ExtractedFile, Extraction, PatchFile, Probe, RepackCtx, RepackFile};
use crate::script::placeholder::{
    denormalize, normalize, ControlCodeSpec, ControlPattern, PlaceholderMap,
};
use crate::script::{ScriptUnit, UnitKind};
use crate::{KonkonError, Result};

use archive::{build_arc, read_arc, ARC_MAGIC};
use layout::ScriptLayout;

/// Phase 0 control codes: ruby open/close (`<R…>` / `</R>`). The full-width
/// space `　` deliberately stays literal. Add more only if a real-file dump
/// surfaces them.
pub const CONTROL_CODES: &[ControlCodeSpec] = &[
    ControlCodeSpec {
        name: "ruby_open",
        pattern: ControlPattern::Regex(r"<R[^>]*>"),
    },
    ControlCodeSpec {
        name: "ruby_close",
        pattern: ControlPattern::Regex(r"</R>"),
    },
];

fn eth_err(detail: impl Into<String>) -> KonkonError {
    KonkonError::Format {
        container: "ethornell",
        detail: detail.into(),
    }
}

/// Lowercase-hex sha256 of a byte slice.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// `true` if `name` (case-insensitive) ends in `.arc`.
fn has_arc_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("arc"))
}

/// Every `*.arc` file directly inside `dir`, sorted by name for determinism.
fn arc_paths(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && has_arc_ext(p))
            .collect(),
        Err(_) => Vec::new(),
    };
    out.sort();
    out
}

/// First `n` bytes of a file, or fewer if it is shorter.
fn read_prefix(path: &Path, n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    match std::fs::File::open(path) {
        Ok(mut f) => {
            let mut filled = 0;
            while filled < n {
                match f.read(&mut buf[filled..]) {
                    Ok(0) => break,
                    Ok(k) => filled += k,
                    Err(_) => break,
                }
            }
            buf.truncate(filled);
            buf
        }
        Err(_) => Vec::new(),
    }
}

/// The `<arc-stem>` part of a `RepackFile::rel_path` (`"data01/00_scene"` → `"data01"`).
fn stem_of(rel_path: &str) -> &str {
    rel_path.split('/').next().unwrap_or(rel_path)
}

/// The `<member>` part of a `RepackFile::rel_path` (`"data01/00_scene"` → `"00_scene"`).
fn member_of(rel_path: &str) -> &str {
    match rel_path.split_once('/') {
        Some((_, m)) => m,
        None => "",
    }
}

/// The Ethornell / BGI engine.
pub struct EthornellEngine;

impl EthornellEngine {
    /// Build the [`ScriptUnit`]s for one parsed script layout, in
    /// `translatable_indices()` order.
    fn units_for(layout: &ScriptLayout) -> Vec<ScriptUnit> {
        layout
            .translatable_indices()
            .iter()
            .enumerate()
            .map(|(ordinal, &si)| {
                let sref = &layout.strings[si];
                let (jp_raw, _, _) = SHIFT_JIS.decode(&sref.bytes);
                let jp_raw = jp_raw.into_owned();
                let (jp_norm, placeholders) = normalize(&jp_raw, CONTROL_CODES);
                ScriptUnit {
                    ordinal,
                    kind: UnitKind::Dialogue,
                    jp_raw,
                    jp_norm,
                    placeholders,
                    speaker_raw: None,
                }
            })
            .collect()
    }

    /// Decompress `raw` if it is DSC, else clone it.
    fn decompressed(raw: &[u8]) -> Result<Vec<u8>> {
        if dsc::is_dsc(raw) {
            dsc::decompress(raw)
        } else {
            Ok(raw.to_vec())
        }
    }
}

impl Engine for EthornellEngine {
    fn id(&self) -> &'static str {
        "ethornell"
    }

    fn display_name(&self) -> &'static str {
        "Ethornell / BGI"
    }

    fn probe(&self, game_dir: &Path) -> Probe {
        if game_dir.join("BGI.hvl").is_file() {
            return Probe::sure("BGI.hvl present");
        }
        let arcs = arc_paths(game_dir);
        let mut saw_arc = false;
        for path in &arcs {
            saw_arc = true;
            if read_prefix(path, ARC_MAGIC.len()) == ARC_MAGIC {
                return Probe::sure(format!(
                    "{} is a BURIKO ARC20 archive",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
        if saw_arc {
            Probe::maybe(0.6, "a .arc file is present but not BURIKO ARC20")
        } else {
            Probe::none()
        }
    }

    fn extract(&self, game_dir: &Path, _work_dir: &Path) -> Result<Extraction> {
        let mut files = Vec::new();
        for arc_path in arc_paths(game_dir) {
            let bytes = std::fs::read(&arc_path)?;
            if bytes.get(..ARC_MAGIC.len()) != Some(ARC_MAGIC) {
                continue;
            }
            let stem = arc_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .ok_or_else(|| eth_err(format!("{} has no file stem", arc_path.display())))?;
            let members = read_arc(&bytes)?;
            for m in &members {
                let decompressed = Self::decompressed(&m.data)?;
                let parsed = match script::parse(&decompressed) {
                    Ok(p) => p,
                    Err(_) => continue, // not a V1 script (ScenarioDB, etc.)
                };
                let units = Self::units_for(&parsed.layout);
                files.push(ExtractedFile {
                    rel_path: format!("{stem}/{}", m.name),
                    encoding: "cp932".to_string(),
                    sha256: sha256_hex(&m.data),
                    layout_json: parsed.layout.to_json()?,
                    units,
                });
            }
        }
        Ok(Extraction { files })
    }

    fn emit(&self) -> EmitKind {
        EmitKind::NativeRepack
    }

    fn repack(&self, ctx: &RepackCtx) -> Result<Vec<PatchFile>> {
        // Group the incoming files by archive stem.
        let mut by_stem: BTreeMap<&str, Vec<&RepackFile>> = BTreeMap::new();
        for rf in &ctx.files {
            by_stem.entry(stem_of(&rf.rel_path)).or_default().push(rf);
        }

        let mut patches = Vec::new();
        for (stem, rfiles) in by_stem {
            let arc_path = ctx.game_dir.join(format!("{stem}.arc"));
            let arc_bytes = std::fs::read(&arc_path)?;
            let mut members = read_arc(&arc_bytes)?;

            for rf in rfiles {
                let member_name = member_of(&rf.rel_path);
                let layout = ScriptLayout::from_json(&rf.layout_json)?;
                let tix = layout.translatable_indices();

                let mut replacements: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
                for unit in &rf.units {
                    if unit.en_norm.is_empty() || unit.en_norm == unit.jp_norm {
                        continue;
                    }
                    let si = *tix.get(unit.ordinal).ok_or_else(|| {
                        eth_err(format!(
                            "{}/{member_name}: unit ordinal {} out of range ({} translatable)",
                            stem,
                            unit.ordinal,
                            tix.len()
                        ))
                    })?;
                    let sref = &layout.strings[si];
                    let map = PlaceholderMap::from_json(&unit.placeholder_map_json)?;
                    let denorm = denormalize(&unit.en_norm, &map)?;
                    let (encoded, _, had_errors) = SHIFT_JIS.encode(&denorm);
                    if had_errors {
                        return Err(eth_err(format!(
                            "{}/{member_name}: unit ordinal {} not representable in CP932: {denorm:?}",
                            stem, unit.ordinal
                        )));
                    }
                    replacements.insert(sref.pool_index, encoded.into_owned());
                }

                let rebuilt = script::rebuild(&layout, &replacements)?;
                let member = members
                    .iter_mut()
                    .find(|m| m.name == member_name)
                    .ok_or_else(|| {
                        eth_err(format!("{stem}.arc has no member named {member_name:?}"))
                    })?;
                member.data = rebuilt; // stored RAW — no DSC re-compress
            }

            let out = build_arc(&members)?;
            patches.push(PatchFile {
                rel_path: format!("{stem}.arc"),
                bytes: out,
            });
        }
        Ok(patches)
    }
}
