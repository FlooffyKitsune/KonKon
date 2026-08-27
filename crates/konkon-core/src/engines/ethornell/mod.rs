//! Ethornell / BGI engine.
//!
//! Stub: real probing, extraction, and repacking land in Tasks 6-9.

pub mod archive;
pub mod dsc;
pub mod layout;
pub mod script;

use super::{EmitKind, Engine, Extraction, PatchFile, Probe, RepackCtx};
use crate::Result;
use std::path::Path;

pub struct EthornellEngine;

impl Engine for EthornellEngine {
    fn id(&self) -> &'static str {
        "ethornell"
    }
    fn display_name(&self) -> &'static str {
        "Ethornell / BGI"
    }

    fn probe(&self, _game_dir: &Path) -> Probe {
        Probe::none() // implemented in Task 9
    }
    fn extract(&self, _game_dir: &Path, _work_dir: &Path) -> Result<Extraction> {
        Ok(Extraction { files: vec![] }) // implemented in Task 9
    }
    fn emit(&self) -> EmitKind {
        EmitKind::NativeRepack
    }
    fn repack(&self, _ctx: &RepackCtx) -> Result<Vec<PatchFile>> {
        Ok(vec![]) // implemented in Task 9
    }
}
