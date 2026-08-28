//! Engine trait, supporting types, and the engine registry.
//!
//! Each VN engine implements [`Engine`] (`probe`/`extract`/`emit`/`repack`).
//! [`registry`] lists the built-in engines and [`detect`] picks the
//! best-matching one for a game directory.

pub mod ethornell;

use crate::script::ScriptUnit;
use crate::{KonkonError, Result};
use std::path::Path;

/// Result of an engine sniffing a game directory.
#[derive(Debug, Clone)]
pub struct Probe {
    /// Match confidence in `0.0..=1.0`.
    pub confidence: f32,
    /// Human-readable note describing what was (or wasn't) matched.
    pub note: String,
}

impl Probe {
    /// No match at all.
    pub fn none() -> Self {
        Self {
            confidence: 0.0,
            note: String::new(),
        }
    }

    /// A definite match.
    pub fn sure(note: impl Into<String>) -> Self {
        Self {
            confidence: 1.0,
            note: note.into(),
        }
    }

    /// A partial match with an explicit confidence (clamped to `0.0..=1.0`).
    pub fn maybe(c: f32, note: impl Into<String>) -> Self {
        Self {
            confidence: c.clamp(0.0, 1.0),
            note: note.into(),
        }
    }
}

/// How an engine applies translated text back to the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitKind {
    /// Rebuild the native archive/script format.
    NativeRepack,
    /// Inject at runtime instead of rewriting game files.
    RuntimeInject,
}

/// One script file produced by [`Engine::extract`].
#[derive(Debug, Clone)]
pub struct ExtractedFile {
    pub rel_path: String,
    pub encoding: String,
    pub sha256: String,
    pub layout_json: String,
    pub units: Vec<ScriptUnit>,
}

/// The full result of [`Engine::extract`].
#[derive(Debug, Clone)]
pub struct Extraction {
    pub files: Vec<ExtractedFile>,
}

/// One translated unit fed back into [`Engine::repack`].
#[derive(Debug, Clone)]
pub struct RepackUnit {
    pub ordinal: usize,
    pub jp_norm: String,
    pub en_norm: String,
    pub placeholder_map_json: String,
}

/// One script file's worth of translated units for [`Engine::repack`].
#[derive(Debug, Clone)]
pub struct RepackFile {
    pub rel_path: String,
    pub layout_json: String,
    pub units: Vec<RepackUnit>,
}

/// Context passed to [`Engine::repack`].
pub struct RepackCtx<'a> {
    pub game_dir: &'a Path,
    pub work_dir: &'a Path,
    pub files: Vec<RepackFile>,
}

/// A patched game file produced by [`Engine::repack`].
#[derive(Debug, Clone)]
pub struct PatchFile {
    pub rel_path: String,
    pub bytes: Vec<u8>,
}

/// The extensibility seam: one implementation per supported VN engine.
pub trait Engine: Sync {
    /// Stable machine identifier (e.g. `"ethornell"`).
    fn id(&self) -> &'static str;
    /// Human-readable name.
    fn display_name(&self) -> &'static str;
    /// Sniff a game directory and report match confidence.
    fn probe(&self, game_dir: &Path) -> Probe;
    /// Extract translatable script from the game into `work_dir`.
    fn extract(&self, game_dir: &Path, work_dir: &Path) -> Result<Extraction>;
    /// How this engine applies translations.
    fn emit(&self) -> EmitKind;
    /// Rebuild patched game files from translated units.
    fn repack(&self, ctx: &RepackCtx) -> Result<Vec<PatchFile>>;
}

impl std::fmt::Debug for dyn Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine").field("id", &self.id()).finish()
    }
}

static ETHORNELL: ethornell::EthornellEngine = ethornell::EthornellEngine;

static REGISTRY: &[&dyn Engine] = &[&ETHORNELL];

/// All built-in engines.
pub fn registry() -> &'static [&'static dyn Engine] {
    REGISTRY
}

/// Pick the highest-confidence engine for `game_dir`.
///
/// Returns [`KonkonError::NoEngineMatch`] if no engine reports
/// `confidence >= 0.5`.
pub fn detect(game_dir: &Path) -> Result<&'static dyn Engine> {
    let mut best: Option<(&'static dyn Engine, Probe)> = None;
    for eng in registry() {
        let p = eng.probe(game_dir);
        let is_better = match &best {
            None => true,
            Some((_, bp)) => p.confidence > bp.confidence,
        };
        if is_better {
            best = Some((*eng, p));
        }
    }
    match best {
        Some((eng, p)) if p.confidence >= 0.5 => Ok(eng),
        _ => Err(KonkonError::NoEngineMatch(game_dir.display().to_string())),
    }
}
