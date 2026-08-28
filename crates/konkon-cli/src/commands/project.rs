//! `konkon project new` — detect the engine and scaffold a project folder.

use anyhow::Context;
use konkon_core::engines::detect;
use konkon_core::project::config::Config;
use konkon_core::project::Project;
use std::path::Path;

pub fn new(game_dir: &Path, name: Option<&str>, into: Option<&Path>) -> anyhow::Result<()> {
    let engine = detect(game_dir).context("engine detection")?;

    let game_name = name.map(str::to_string).unwrap_or_else(|| {
        game_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Game")
            .to_string()
    });
    let parent = into.unwrap_or_else(|| Path::new("."));

    let proj = Project::create(parent, &game_name)?;
    let cfg = Config {
        engine_id: Some(engine.id().to_string()),
        game_dir: Some(
            game_dir
                .canonicalize()
                .unwrap_or_else(|_| game_dir.to_path_buf()),
        ),
    };
    cfg.save(&proj.root)?;

    println!("created {} (engine: {})", proj.root.display(), engine.id());
    Ok(())
}
