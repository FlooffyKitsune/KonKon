use crate::{KonkonError, Result};
use encoding_rs::SHIFT_JIS;

pub const ARC_MAGIC: &[u8] = b"BURIKO ARC20";
const ENTRY_SIZE: usize = 0x80;
const NAME_LEN: usize = 0x60;
const RESERVED_LEN: usize = 0x18;
const INDEX_START: usize = 0x10;

/// One archive member.
///
/// `name` is a lossy CP932 decode of the name field, kept as a convenience for
/// display and `rel_path` building. `raw_name` and `reserved` preserve the exact
/// on-disk bytes so [`build_arc`] can reproduce an untouched archive
/// byte-for-byte (the offset/size words are always recomputed).
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    /// The exact `NAME_LEN` (0x60) bytes of the name field.
    pub raw_name: Vec<u8>,
    /// The exact `RESERVED_LEN` (0x18) reserved bytes that follow offset+size.
    pub reserved: [u8; RESERVED_LEN],
    pub data: Vec<u8>,
}

impl Member {
    /// Build a member from a display name and its data, encoding the name to a
    /// zero-padded CP932 name field and zeroing the reserved bytes. Used by the
    /// repacker and tests; [`read_arc`] populates the raw fields verbatim instead.
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Self {
        let name = name.into();
        let (enc, _, _) = SHIFT_JIS.encode(&name);
        let mut raw_name = vec![0u8; NAME_LEN];
        let n = enc.len().min(NAME_LEN);
        raw_name[..n].copy_from_slice(&enc[..n]);
        Self {
            name,
            raw_name,
            reserved: [0u8; RESERVED_LEN],
            data,
        }
    }
}

fn read_u32(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or(KonkonError::Format {
            container: "arc",
            detail: format!("truncated at {at}"),
        })
}

pub fn read_arc(bytes: &[u8]) -> Result<Vec<Member>> {
    if bytes.get(..ARC_MAGIC.len()) != Some(ARC_MAGIC) {
        return Err(KonkonError::Format {
            container: "arc",
            detail: "bad magic (expected BURIKO ARC20)".into(),
        });
    }
    let count = read_u32(bytes, 0x0C)? as usize;

    // Validate the index fits before allocating anything sized from `count`:
    // a bogus u32 (e.g. 0xFFFFFFFF) must not drive a huge allocation.
    let index_end = count
        .checked_mul(ENTRY_SIZE)
        .and_then(|n| INDEX_START.checked_add(n))
        .ok_or(KonkonError::Format {
            container: "arc",
            detail: format!("index size for {count} entries overflows"),
        })?;
    if index_end > bytes.len() {
        return Err(KonkonError::Format {
            container: "arc",
            detail: format!(
                "index claims {count} entries but file is {} bytes",
                bytes.len()
            ),
        });
    }

    let data_start = index_end;
    let mut members = Vec::with_capacity(count.min(bytes.len() / ENTRY_SIZE));
    for i in 0..count {
        let e = INDEX_START + i * ENTRY_SIZE;
        let name_bytes = bytes.get(e..e + NAME_LEN).ok_or(KonkonError::Format {
            container: "arc",
            detail: format!("truncated index entry at {e}"),
        })?;
        let end = name_bytes.iter().position(|&c| c == 0).unwrap_or(NAME_LEN);
        let (name, _, _) = SHIFT_JIS.decode(&name_bytes[..end]);
        let off = read_u32(bytes, e + NAME_LEN)? as usize;
        let size = read_u32(bytes, e + NAME_LEN + 4)? as usize;
        let reserved: [u8; RESERVED_LEN] = bytes
            .get(e + NAME_LEN + 8..e + NAME_LEN + 8 + RESERVED_LEN)
            .ok_or(KonkonError::Format {
                container: "arc",
                detail: format!("truncated reserved bytes at {e}"),
            })?
            .try_into()
            .unwrap();
        let start = data_start.checked_add(off).ok_or(KonkonError::Format {
            container: "arc",
            detail: format!("member '{name}' offset overflows"),
        })?;
        let slice = start
            .checked_add(size)
            .and_then(|s_end| bytes.get(start..s_end))
            .ok_or(KonkonError::Format {
                container: "arc",
                detail: format!("member '{name}' data out of range"),
            })?;
        members.push(Member {
            name: name.into_owned(),
            raw_name: name_bytes.to_vec(),
            reserved,
            data: slice.to_vec(),
        });
    }
    Ok(members)
}

pub fn build_arc(members: &[Member]) -> Result<Vec<u8>> {
    let count = members.len();
    let mut out = Vec::new();
    out.extend_from_slice(ARC_MAGIC);
    out.extend_from_slice(&(count as u32).to_le_bytes());

    let mut data = Vec::new();
    for m in members {
        if m.raw_name.len() != NAME_LEN {
            return Err(KonkonError::Format {
                container: "arc",
                detail: format!(
                    "member '{}' raw_name is {} bytes, expected {NAME_LEN}",
                    m.name,
                    m.raw_name.len()
                ),
            });
        }
        out.extend_from_slice(&m.raw_name);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(m.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&m.reserved);
        data.extend_from_slice(&m.data);
    }
    out.extend_from_slice(&data);
    Ok(out)
}
