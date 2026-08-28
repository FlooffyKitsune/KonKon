//! Subcommand implementations. Each is a thin adapter over `konkon_core`.

pub mod extract;
pub mod patch;
pub mod project;
pub mod script_io;
pub mod verify;

use anyhow::anyhow;
use konkon_core::engines::{registry, Engine};
use konkon_core::project::config::Config;

/// Resolve `(game_dir, engine)` from a project config, erroring cleanly if the
/// config is missing either field or names an unknown engine.
fn resolved(cfg: &Config) -> anyhow::Result<(std::path::PathBuf, &'static dyn Engine)> {
    let game_dir = cfg
        .game_dir
        .clone()
        .ok_or_else(|| anyhow!("project config has no game_dir"))?;
    let engine_id = cfg
        .engine_id
        .clone()
        .ok_or_else(|| anyhow!("project config has no engine_id"))?;
    let engine = registry()
        .iter()
        .copied()
        .find(|e| e.id() == engine_id)
        .ok_or_else(|| anyhow!("unknown engine {engine_id}"))?;
    Ok((game_dir, engine))
}
