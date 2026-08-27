//! On-disk layout of a parsed `BurikoCompiledScriptVer1.00` script.
//!
//! [`ScriptLayout`] is the hand-off between the parser ([`super::script::parse`])
//! and the rebuilder (Task 8): it records where the bytecode ends, the original
//! string pool in file order, every string reference site in the bytecode, and a
//! verbatim copy of everything up to the string pool. A rebuild walks
//! [`ScriptLayout::pool`] in order (original bytes where a `pool_index` was not
//! translated, replacement bytes where it was), so a no-op rebuild reproduces the
//! pool byte-for-byte and every operand is left unchanged.

use serde::{Deserialize, Serialize};

use crate::Result;

/// How a pool string is used, which decides whether it is translated.
///
/// Mirrors VNTextPatch's `ScriptStringType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrKind {
    /// Function names, voice ids, asset names, empty strings — preserved as-is.
    Internal,
    /// Dialogue / narration / choice text — translatable.
    Message,
    /// Speaker name — translatable.
    CharacterName,
}

impl StrKind {
    /// `true` for the kinds that are surfaced as translation units.
    pub fn is_translatable(self) -> bool {
        !matches!(self, StrKind::Internal)
    }
}

/// One zero-terminated string in the original pool, in file order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolEntry {
    /// Absolute offset of the string's first byte in the decompressed script.
    pub offset: usize,
    /// The CP932 bytes of the string, NUL excluded.
    pub bytes: Vec<u8>,
}

/// One string-address operand encountered during disassembly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptStringRef {
    /// Absolute offset of the `i32` address operand within the script bytes.
    /// A rebuild rewrites the `i32` here to `new_text_offset - code_offset`.
    pub operand_offset: usize,
    /// Absolute offset of the string in the original script bytes
    /// (`code_offset + address`). Always equal to `pool[pool_index].offset`.
    pub text_offset: usize,
    /// Index into [`ScriptLayout::pool`] of the string this operand points at.
    /// Several refs may share one `pool_index` (deduplicated strings).
    pub pool_index: usize,
    /// Classification driving translate-vs-preserve.
    pub kind: StrKind,
    /// The zero-terminated CP932 bytes at `text_offset`, NUL excluded, captured
    /// at parse time. Always equal to `pool[pool_index].bytes`.
    pub bytes: Vec<u8>,
}

/// The parsed layout of a V1 compiled script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLayout {
    /// `magic.len() + header_size` — first byte of the bytecode.
    pub code_offset: usize,
    /// Length of the bytecode region walked by the disassembler.
    pub code_length: usize,
    /// The script bytes `[0, code_offset + code_length)`, copied verbatim on a
    /// rebuild (header + bytecode, including any embedded message records).
    pub prefix: Vec<u8>,
    /// The original string pool `[code_offset + code_length .. EOF)` split on
    /// NUL, in file order. Every referenced string is one of these entries.
    pub pool: Vec<PoolEntry>,
    /// Any bytes after the last NUL of the pool region that do not form a
    /// zero-terminated string (normally empty — the pool ends on a NUL).
    pub pool_tail: Vec<u8>,
    /// Every string reference site, in the order the disassembler emitted them.
    /// This order is the translation-unit order.
    pub strings: Vec<ScriptStringRef>,
}

impl ScriptLayout {
    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Deserialize from JSON produced by [`ScriptLayout::to_json`].
    pub fn from_json(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s)?)
    }

    /// Indices into [`ScriptLayout::strings`] whose `kind` is translatable, in
    /// order. This is the translation-unit ordering consumed by Tasks 8/9:
    /// `translatable_indices()[ordinal]` is the `strings` index of the n-th
    /// unit, and `strings[that].pool_index` is the pool entry to replace.
    pub fn translatable_indices(&self) -> Vec<usize> {
        self.strings
            .iter()
            .enumerate()
            .filter(|(_, s)| s.kind.is_translatable())
            .map(|(i, _)| i)
            .collect()
    }
}
