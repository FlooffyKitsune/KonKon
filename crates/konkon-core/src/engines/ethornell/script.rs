//! Parse a decompressed `BurikoCompiledScriptVer1.00` script into a
//! [`ScriptLayout`].
//!
//! Port of VNTextPatch's `EthornellScript.Load` for the V1 path: run the
//! [`super::disasm`] disassembler, split the original string pool on NUL, then
//! bind every emitted string reference to its pool entry. The input must be the
//! *decompressed* script bytes (post-`dsc::decompress`).

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::Result;

use super::disasm::{self, format_err, resolve_string_address, MAGIC};
use super::layout::{PoolEntry, ScriptLayout, ScriptStringRef};

/// A parsed script: just its [`ScriptLayout`], which carries everything Tasks
/// 8/9 need to rebuild and to emit translation units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedScript {
    pub layout: ScriptLayout,
}

/// Parse decompressed V1 script bytes.
///
/// # Errors
///
/// [`crate::KonkonError::Format`] with `container: "script"` for a bad magic, a
/// truncated header, an out-of-bounds read, an opcode absent from the V1
/// operand-template map, a string reference that does not land on a pool-string
/// boundary, or any structural inconsistency. Never panics.
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

    let prefix_end = code_offset + code_length;
    let prefix = decompressed
        .get(..prefix_end)
        .ok_or_else(|| format_err("prefix range past end of script"))?
        .to_vec();

    // The original string pool, split on NUL in file order. Independent of the
    // disassembly: a straight linear walk of [prefix_end, EOF).
    let (pool, pool_tail) = split_pool(decompressed, prefix_end);
    let offset_to_index: HashMap<usize, usize> = pool
        .iter()
        .enumerate()
        .map(|(i, e)| (e.offset, i))
        .collect();

    let mut strings = Vec::with_capacity(disassembly.sites.len());
    for site in &disassembly.sites {
        let text_offset = resolve_string_address(decompressed, code_offset, site.address)?;
        let pool_index = *offset_to_index.get(&text_offset).ok_or_else(|| {
            format_err(format!(
                "string reference at operand offset {} points to {text_offset}, \
                 which is not a pool-string boundary",
                site.operand_offset
            ))
        })?;
        strings.push(ScriptStringRef {
            operand_offset: site.operand_offset,
            text_offset,
            pool_index,
            kind: site.kind,
            bytes: pool[pool_index].bytes.clone(),
        });
    }

    Ok(ParsedScript {
        layout: ScriptLayout {
            code_offset,
            code_length,
            prefix,
            pool,
            pool_tail,
            strings,
        },
    })
}

/// Rebuild a decompressed V1 script with translated string-pool entries.
///
/// `replacements` maps a `pool_index` (an index into [`ScriptLayout::pool`]) to
/// the new CP932 bytes for that entry (NUL excluded). Only a `pool_index`
/// referenced by at least one non-[`StrKind::Internal`](super::layout::StrKind)
/// [`ScriptStringRef`] may appear. A missing key keeps `pool[i].bytes`
/// unchanged. No deduplication is performed: the pool is emitted in original
/// order so `rebuild(layout, &BTreeMap::new())` reproduces the input decompressed
/// script byte-for-byte.
///
/// The returned bytes are the rebuilt **decompressed** script (not DSC).
///
/// # Errors
///
/// [`crate::KonkonError::Format`] with `container: "script"` if a replacement
/// value contains a NUL byte, if a key is not referenced by any non-`Internal`
/// string reference (including keys outside the pool range), or if an operand
/// rewrite would fall outside the copied prefix. Never panics.
pub fn rebuild(layout: &ScriptLayout, replacements: &BTreeMap<usize, Vec<u8>>) -> Result<Vec<u8>> {
    // pool_index values that appear on at least one translatable string ref.
    let translatable: HashSet<usize> = layout
        .strings
        .iter()
        .filter(|s| s.kind.is_translatable())
        .map(|s| s.pool_index)
        .collect();

    for (&key, value) in replacements {
        if !translatable.contains(&key) {
            return Err(format_err(format!(
                "replacement key {key} is not referenced by any translatable \
                 (non-Internal) string"
            )));
        }
        if value.contains(&0) {
            return Err(format_err(format!(
                "replacement for pool index {key} contains a NUL byte"
            )));
        }
    }

    // 1. Emit the new pool by walking `layout.pool` in original order.
    let pool_base = layout
        .code_offset
        .checked_add(layout.code_length)
        .ok_or_else(|| format_err("code_offset + code_length overflow"))?;
    let mut pool_out: Vec<u8> = Vec::new();
    let mut new_offset: Vec<usize> = Vec::with_capacity(layout.pool.len());
    for (i, entry) in layout.pool.iter().enumerate() {
        let text = replacements.get(&i).unwrap_or(&entry.bytes);
        new_offset.push(pool_base + pool_out.len());
        pool_out.extend_from_slice(text);
        pool_out.push(0);
    }
    pool_out.extend_from_slice(&layout.pool_tail);

    // 2. Copy the prefix (header + bytecode) verbatim.
    let mut out = layout.prefix.clone();

    // 3. Repatch every string-address operand (including Internal refs).
    for s in &layout.strings {
        let abs = *new_offset.get(s.pool_index).ok_or_else(|| {
            format_err(format!(
                "string ref points at pool_index {} but the pool has {} entries",
                s.pool_index,
                new_offset.len()
            ))
        })?;
        let rel = abs
            .checked_sub(layout.code_offset)
            .ok_or_else(|| format_err("rebuilt string offset precedes code_offset"))?;
        let rel = u32::try_from(rel)
            .map_err(|_| format_err("rebuilt string offset exceeds u32 range"))?;
        let end = s
            .operand_offset
            .checked_add(4)
            .ok_or_else(|| format_err("operand offset + 4 overflow"))?;
        let out_len = out.len();
        let slot = out.get_mut(s.operand_offset..end).ok_or_else(|| {
            format_err(format!(
                "operand offset {} is outside the copied prefix ({out_len} bytes)",
                s.operand_offset,
            ))
        })?;
        slot.copy_from_slice(&rel.to_le_bytes());
    }

    // 4. Append the rebuilt pool.
    out.extend_from_slice(&pool_out);
    Ok(out)
}

/// Split `data[pool_start..]` into zero-terminated entries plus any trailing
/// bytes with no NUL.
fn split_pool(data: &[u8], pool_start: usize) -> (Vec<PoolEntry>, Vec<u8>) {
    let mut pool = Vec::new();
    let mut cur = pool_start;
    let region = &data[pool_start.min(data.len())..];
    let mut rest = region;
    while let Some(nul) = rest.iter().position(|&b| b == 0) {
        pool.push(PoolEntry {
            offset: cur,
            bytes: rest[..nul].to_vec(),
        });
        cur += nul + 1;
        rest = &rest[nul + 1..];
    }
    (pool, rest.to_vec())
}
