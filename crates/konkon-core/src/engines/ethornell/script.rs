//! Parse a decompressed `BurikoCompiledScriptVer1.00` script into a
//! [`ScriptLayout`].
//!
//! Port of VNTextPatch's `EthornellScript.Load` for the V1 path: run the
//! [`super::disasm`] disassembler, then resolve every emitted string reference
//! site to its zero-terminated CP932 bytes. The input must be the *decompressed*
//! script bytes (post-`dsc::decompress`).

use crate::{KonkonError, Result};

use super::disasm::{self, MAGIC};
use super::layout::{ScriptLayout, ScriptStringRef};

/// A parsed script: just its [`ScriptLayout`], which carries everything Tasks
/// 8/9 need to rebuild and to emit translation units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedScript {
    pub layout: ScriptLayout,
}

fn format_err(detail: impl Into<String>) -> KonkonError {
    KonkonError::Format {
        container: "script",
        detail: detail.into(),
    }
}

/// Parse decompressed V1 script bytes.
///
/// # Errors
///
/// [`KonkonError::Format`] with `container: "script"` for a bad magic, a
/// truncated header, an out-of-bounds read, an opcode absent from the V1
/// operand-template map, or any structural inconsistency. Never panics.
pub fn parse(decompressed: &[u8]) -> Result<ParsedScript> {
    if decompressed.get(..MAGIC.len()) != Some(MAGIC) {
        return Err(format_err(
            "bad magic (expected \"BurikoCompiledScriptVer1.00\")",
        ));
    }

    let header_size_bytes = decompressed
        .get(MAGIC.len()..MAGIC.len() + 4)
        .ok_or_else(|| format_err("header truncated: no header_size field"))?;
    let header_size = i32::from_le_bytes(header_size_bytes.try_into().unwrap());
    if header_size < 0 {
        return Err(format_err(format!("negative header_size {header_size}")));
    }
    let code_offset = MAGIC
        .len()
        .checked_add(header_size as usize)
        .ok_or_else(|| format_err("header_size overflow"))?;
    if code_offset > decompressed.len() {
        return Err(format_err(format!(
            "code_offset {code_offset} past end of script ({} bytes)",
            decompressed.len()
        )));
    }

    let disassembly = disasm::disassemble(decompressed, code_offset)?;
    let code_length = disassembly
        .end_pos
        .checked_sub(code_offset)
        .ok_or_else(|| format_err("disassembler ended before code_offset"))?;

    let mut strings = Vec::with_capacity(disassembly.sites.len());
    for site in &disassembly.sites {
        let abs = code_offset as i64 + site.address as i64;
        if abs < 0 || abs as u64 >= decompressed.len() as u64 {
            return Err(format_err(format!(
                "string address {} resolves outside the script",
                site.address
            )));
        }
        let text_offset = abs as usize;
        let bytes = disasm::read_zero_terminated(decompressed, text_offset)?;
        strings.push(ScriptStringRef {
            operand_offset: site.operand_offset,
            text_offset,
            kind: site.kind,
            bytes,
        });
    }

    let prefix_end = code_offset + code_length;
    let prefix = decompressed
        .get(..prefix_end)
        .ok_or_else(|| format_err("prefix range past end of script"))?
        .to_vec();

    Ok(ParsedScript {
        layout: ScriptLayout {
            code_offset,
            code_length,
            prefix,
            strings,
        },
    })
}
