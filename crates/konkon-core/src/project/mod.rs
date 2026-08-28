pub mod config;
pub mod store;

use crate::Result;
use std::path::{Path, PathBuf};

pub struct Project {
    pub root: PathBuf,
}

impl Project {
    pub fn create(parent: &Path, game_name: &str) -> Result<Self> {
        let root = parent.join(format!("{game_name}.konkon"));
        if root.join("project.db").exists() {
            return Err(crate::KonkonError::Other(format!(
                "project already exists at {}",
                root.display()
            )));
        }
        for sub in ["", "extracted", "patch", "backups"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        config::Config::default().save(&root)?;
        let _ = store::Store::open(&root.join("project.db"))?; // runs migrations
        Ok(Self { root })
    }

    pub fn open(root: &Path) -> Result<Self> {
        if !root.join("project.db").is_file() {
            return Err(crate::KonkonError::Other(format!(
                "no project.db under {}",
                root.display()
            )));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    pub fn store(&self) -> Result<store::Store> {
        store::Store::open(&self.root.join("project.db"))
    }

    pub fn config(&self) -> Result<config::Config> {
        config::Config::load(&self.root)
    }
}
