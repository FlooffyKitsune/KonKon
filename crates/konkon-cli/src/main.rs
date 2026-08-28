//! `konkon` — the command-line surface for KonKon.
//!
//! A thin clap wrapper: every subcommand parses arguments, calls into
//! [`konkon_core`], and presents the result. No format or round-trip logic
//! lives here.

mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "konkon", version, about = "Translate Japanese visual novels")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Manage translation projects.
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },
    /// Extract scripts into the project database.
    Extract { project: PathBuf },
    /// Export translatable lines to JSONL.
    Export {
        project: PathBuf,
        /// Only export units from this `rel_path`.
        #[arg(long)]
        file: Option<String>,
        /// Destination JSONL file.
        #[arg(long)]
        out: PathBuf,
    },
    /// Import translated lines from JSONL.
    Import { project: PathBuf, jsonl: PathBuf },
    /// Prove the extract/rebuild round-trip is lossless.
    Verify { project: PathBuf },
    /// Build the patch artifact from current translations.
    Build { project: PathBuf },
    /// Apply the patch to the game (backs up originals first).
    Apply { project: PathBuf },
    /// Restore the game to its original state.
    Revert { project: PathBuf },
}

#[derive(Subcommand)]
enum ProjectAction {
    /// Detect the engine and scaffold a project folder.
    New {
        game_dir: PathBuf,
        /// Project name (defaults to the game directory's name).
        #[arg(long)]
        name: Option<String>,
        /// Parent directory for `<name>.konkon/` (defaults to the cwd).
        #[arg(long)]
        into: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Project {
            action:
                ProjectAction::New {
                    game_dir,
                    name,
                    into,
                },
        } => commands::project::new(&game_dir, name.as_deref(), into.as_deref()),
        Cmd::Extract { project } => commands::extract::run(&project),
        Cmd::Export { project, file, out } => {
            commands::script_io::export(&project, file.as_deref(), &out)
        }
        Cmd::Import { project, jsonl } => commands::script_io::import(&project, &jsonl),
        Cmd::Verify { project } => commands::verify::run(&project),
        Cmd::Build { project } => commands::patch::build(&project),
        Cmd::Apply { project } => commands::patch::apply(&project),
        Cmd::Revert { project } => commands::patch::revert(&project),
    }
}
