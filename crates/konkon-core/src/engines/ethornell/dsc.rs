//! Ethornell / BGI `DSC FORMAT 1.00` decompressor.
//!
//! Every BGI scenario script (and many other archive members) is stored
//! DSC-compressed: a 32-byte header, a 512-entry obfuscated code-length table,
//! then an MSB-first Huffman + LZ bitstream.
//!
//! This is a faithful port of the `arc-reader` project's `dsc_decrypt` +
//! `hash_update` C reference, with the two documented corrections applied:
//!
//! * the decode loop is output-size driven (`while dst < decompressed_size`),
//!   not input driven — some members legitimately exhaust the packed bitstream
//!   one token before the output is full;
//! * reads past the end of the packed bitstream yield `0`, never an error or a
//!   panic.
//!
//! All of the C reference's `uint32_t` arithmetic is reproduced with Rust
//! `wrapping_*` operations on `u32`.

use crate::{KonkonError, Result};

/// The 16-byte signature at the start of every DSC blob.
pub const MAGIC: &[u8] = b"DSC FORMAT 1.00\0";

/// The 15 bytes of [`MAGIC`] that are actually checked; the 16th byte is lenient.
const MAGIC_PREFIX: &[u8] = b"DSC FORMAT 1.00";

const HEADER_LEN: usize = 0x20;
const TABLE_LEN: usize = 512;
const BITSTREAM_START: usize = HEADER_LEN + TABLE_LEN; // 0x220
/// Upper bound on Huffman node / work-vector indices (512 leaves + internals).
const NODE_CAP: usize = 1024;
/// Hard ceiling on the header-declared decompressed size. The field is an
/// attacker-controllable `u32`; real BGI scripts are well under 10 MiB, so this
/// leaves ample headroom while preventing a corrupt member from driving a
/// multi-gigabyte allocation (which aborts the process on Windows rather than
/// returning an error).
const MAX_DECOMPRESSED: usize = 128 * 1024 * 1024;

fn format_err(detail: impl Into<String>) -> KonkonError {
    KonkonError::Format {
        container: "dsc",
        detail: detail.into(),
    }
}

/// Returns `true` when `bytes` begins with the `DSC FORMAT 1.00` signature.
///
/// Only the 15-byte prefix is checked; the 16th byte (a NUL in well-formed
/// blobs) is ignored.
pub fn is_dsc(bytes: &[u8]) -> bool {
    bytes.get(..MAGIC_PREFIX.len()) == Some(MAGIC_PREFIX)
}

/// BGI key-stream step. Faithful port of the reference `hash_update`: every
/// operation is 32-bit wrapping. Returns the low 15 bits of the intermediate
/// `eax`; callers use the low 8 bits as the key byte.
fn hash_update(hash: &mut u32) -> u32 {
    let h = *hash;
    let lo = h & 0xFFFF;
    let hi = (h >> 16) & 0xFFFF;
    let edx = 20021u32.wrapping_mul(lo);
    let eax = 20021u32
        .wrapping_mul(hi)
        .wrapping_add(346u32.wrapping_mul(h))
        .wrapping_add((edx >> 16) & 0xFFFF);
    *hash = ((eax & 0xFFFF) << 16)
        .wrapping_add(edx & 0xFFFF)
        .wrapping_add(1);
    eax & 0x7FFF
}

/// Canonical Huffman decode tree, built from the de-obfuscated code lengths.
struct Tree {
    has_childs: [bool; NODE_CAP],
    leaf_value: [u32; NODE_CAP],
    childs: [[u32; 2]; NODE_CAP],
}

impl Tree {
    /// Build the tree from the 512-byte obfuscated code-length table.
    ///
    /// Mirrors the reference's double-buffered breadth-first construction: for
    /// each code length `nn` in turn, the symbols of that length become leaves
    /// and the remaining slots are split into internal nodes for the next level.
    fn build(table: &[u8], hash: &mut u32) -> Result<Self> {
        // De-obfuscate the code lengths and collect the used symbols as
        // `(length << 16) | symbol`, then sort into canonical `(length, symbol)`
        // order. Length 0 means "unused".
        let mut buffer: Vec<u32> = Vec::with_capacity(TABLE_LEN);
        for (n, &raw) in table.iter().enumerate() {
            let key = hash_update(hash) as u8;
            let code_len = raw.wrapping_sub(key);
            if code_len != 0 {
                buffer.push((u32::from(code_len) << 16) | n as u32);
            }
        }
        buffer.sort_unstable();

        let mut tree = Tree {
            has_childs: [false; NODE_CAP],
            leaf_value: [0; NODE_CAP],
            childs: [[0; 2]; NODE_CAP],
        };
        let mut vector0 = [0u32; NODE_CAP];

        let mut nn: u32 = 0;
        let mut toggle: usize = 0x200;
        let mut dec0: u32 = 1;
        let mut value_set: u32 = 1;
        let mut read_ptr: usize = 0;
        let mut bc: usize = 0;
        let mut guard: u32 = 0;

        while bc < buffer.len() {
            guard += 1;
            if guard > (NODE_CAP as u32) * 2 {
                return Err(format_err("huffman table build did not converge"));
            }

            let write_init = toggle;
            let mut write_ptr = toggle;
            let mut group_count: u32 = 0;

            // Symbols whose code length is exactly `nn` become leaves.
            while bc < buffer.len() && nn == (buffer[bc] >> 16) {
                let node = *vector0
                    .get(read_ptr)
                    .ok_or_else(|| format_err("huffman leaf index out of range"))?
                    as usize;
                if node >= NODE_CAP {
                    return Err(format_err("huffman node index out of range"));
                }
                tree.has_childs[node] = false;
                tree.leaf_value[node] = buffer[bc] & 0x1FF;
                bc += 1;
                read_ptr += 1;
                group_count += 1;
            }

            let next_dec0 = 2u32.wrapping_mul(dec0.wrapping_sub(group_count));

            if group_count > dec0 {
                return Err(format_err("over-subscribed huffman code lengths"));
            }
            if group_count < dec0 {
                dec0 -= group_count;
                for _ in 0..dec0 {
                    let node = *vector0
                        .get(read_ptr)
                        .ok_or_else(|| format_err("huffman node index out of range"))?
                        as usize;
                    if node >= NODE_CAP {
                        return Err(format_err("huffman node index out of range"));
                    }
                    tree.has_childs[node] = true;
                    for m in 0..2 {
                        if value_set as usize >= NODE_CAP {
                            return Err(format_err("huffman tree too large"));
                        }
                        if write_ptr >= NODE_CAP {
                            return Err(format_err("huffman work vector overflow"));
                        }
                        tree.childs[node][m] = value_set;
                        vector0[write_ptr] = value_set;
                        write_ptr += 1;
                        value_set += 1;
                    }
                    read_ptr += 1;
                }
            }

            dec0 = next_dec0;
            read_ptr = write_init;
            toggle ^= 0x200;
            nn += 1;
            if nn > 255 {
                return Err(format_err("huffman code length exceeds 255"));
            }
        }

        Ok(tree)
    }
}

/// Decompress a full `DSC FORMAT 1.00` blob (header + table + bitstream).
///
/// Returns exactly `decompressed_size` bytes (the `u32` at offset `0x14`), or a
/// [`KonkonError::Format`] with `container: "dsc"` if the blob is malformed.
pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < HEADER_LEN {
        return Err(format_err(format!(
            "header truncated: {} bytes, need at least {HEADER_LEN}",
            bytes.len()
        )));
    }
    if !is_dsc(bytes) {
        return Err(format_err("bad magic (expected \"DSC FORMAT 1.00\")"));
    }
    let table = bytes.get(HEADER_LEN..BITSTREAM_START).ok_or_else(|| {
        format_err(format!(
            "code-length table truncated: {} bytes, need at least {BITSTREAM_START}",
            bytes.len()
        ))
    })?;

    let mut hash = u32::from_le_bytes(bytes[0x10..0x14].try_into().unwrap());
    let out_size = u32::from_le_bytes(bytes[0x14..0x18].try_into().unwrap()) as usize;

    if out_size > MAX_DECOMPRESSED {
        return Err(format_err(format!(
            "implausible decompressed size {out_size} (cap {MAX_DECOMPRESSED})"
        )));
    }

    let tree = Tree::build(table, &mut hash)?;

    let crypt = &bytes[BITSTREAM_START..];
    let fetch = |src_ptr: &mut usize| -> u32 {
        let b = crypt.get(*src_ptr).copied().unwrap_or(0);
        *src_ptr += 1;
        u32::from(b)
    };

    let mut out = vec![0u8; out_size];
    let mut src_ptr: usize = 0;
    let mut dst_ptr: usize = 0;
    let mut bits: u32 = 0;
    let mut nbits: u32 = 0;

    while dst_ptr < out_size {
        // Walk the Huffman tree, MSB-first, one bit at a time.
        let mut nentry: usize = 0;
        while tree.has_childs[nentry] {
            if nbits == 0 {
                nbits = 8;
                bits = fetch(&mut src_ptr);
            }
            nentry = tree.childs[nentry][((bits >> 7) & 1) as usize] as usize;
            if nentry >= NODE_CAP {
                return Err(format_err("huffman traversal index out of range"));
            }
            nbits -= 1;
            bits = (bits << 1) & 0xFF;
        }

        let info = tree.leaf_value[nentry] & 0xFFFF;

        if (info >> 8) & 0xFF == 1 {
            // Match token: variable-length offset field spanning `nbits` leftover
            // bits plus whole bytes from the stream.
            let mut cvalue = if nbits > 0 { bits >> (8 - nbits) } else { 0 };
            let mut nbits2 = nbits;
            if nbits < 12 {
                let nbytes = ((11 - nbits) >> 3) + 1;
                for _ in 0..nbytes {
                    cvalue = fetch(&mut src_ptr).wrapping_add(cvalue << 8);
                    nbits2 += 8;
                }
            }
            if nbits2 < 12 {
                return Err(format_err("malformed match token (degenerate tree)"));
            }
            let extra = nbits2 - 12;
            nbits = extra;
            bits = (cvalue << (8 - extra)) & 0xFF;

            let offset = (cvalue >> extra).wrapping_add(2);
            let count = (info & 0xFF) + 2;
            let mut read_at: i64 = dst_ptr as i64 - offset as i64;
            let mut remaining = count;
            while remaining > 0 {
                if dst_ptr >= out_size {
                    return Err(format_err("decompressed output overran the declared size"));
                }
                let byte = if read_at >= 0 {
                    out[read_at as usize]
                } else {
                    0
                };
                out[dst_ptr] = byte;
                dst_ptr += 1;
                read_at += 1;
                remaining -= 1;
            }
        } else {
            out[dst_ptr] = (info & 0xFF) as u8;
            dst_ptr += 1;
        }
    }

    if dst_ptr != out_size {
        return Err(format_err(format!(
            "decoded {dst_ptr} bytes, header declared {out_size}"
        )));
    }
    Ok(out)
}
