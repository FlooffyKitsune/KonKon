//! Script model: [`ScriptUnit`] / [`UnitKind`] plus the [`placeholder`] submodule
//! that converts VN control codes to/from stable `{0}`, `{1}` placeholders.

pub mod placeholder;

use placeholder::PlaceholderMap;

/// Semantic category of a translatable script unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Dialogue,
    Narration,
    Choice,
    System,
}

impl UnitKind {
    /// Stable lowercase tag used for storage / serialization.
    pub fn as_str(self) -> &'static str {
        match self {
            UnitKind::Dialogue => "dialogue",
            UnitKind::Narration => "narration",
            UnitKind::Choice => "choice",
            UnitKind::System => "system",
        }
    }

    /// Inverse of [`UnitKind::as_str`]; returns `None` for an unknown tag.
    #[allow(clippy::should_implement_trait)] // task brief mandates this exact name/signature
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "dialogue" => UnitKind::Dialogue,
            "narration" => UnitKind::Narration,
            "choice" => UnitKind::Choice,
            "system" => UnitKind::System,
            _ => return None,
        })
    }
}

/// A single translatable unit extracted from a VN script.
#[derive(Debug, Clone)]
pub struct ScriptUnit {
    pub ordinal: usize,
    pub kind: UnitKind,
    pub jp_raw: String,
    pub jp_norm: String,
    pub placeholders: PlaceholderMap,
    pub speaker_raw: Option<String>,
}
