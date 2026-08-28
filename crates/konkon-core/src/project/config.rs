use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub engine_id: Option<String>,
    pub game_dir: Option<PathBuf>,
}

impl Config {
    pub fn load(root: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(root.join("config.toml"))?;
        toml::from_str(&text).map_err(|e| crate::KonkonError::Other(e.to_string()))
    }

    pub fn save(&self, root: &Path) -> Result<()> {
        let text =
            toml::to_string_pretty(self).map_err(|e| crate::KonkonError::Other(e.to_string()))?;
        std::fs::write(root.join("config.toml"), text)?;
        Ok(())
    }
}
