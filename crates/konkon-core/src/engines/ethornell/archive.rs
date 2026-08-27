use crate::{KonkonError, Result};
use encoding_rs::SHIFT_JIS;

pub const ARC_MAGIC: &[u8] = b"BURIKO ARC20";
const ENTRY_SIZE: usize = 0x80;
const NAME_LEN: usize = 0x60;
const INDEX_START: usize = 0x10;

#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub data: Vec<u8>,
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
    let data_start = INDEX_START + count * ENTRY_SIZE;
    let mut members = Vec::with_capacity(count);
    for i in 0..count {
        let e = INDEX_START + i * ENTRY_SIZE;
        let name_bytes = &bytes[e..e + NAME_LEN];
        let end = name_bytes.iter().position(|&c| c == 0).unwrap_or(NAME_LEN);
        let (name, _, _) = SHIFT_JIS.decode(&name_bytes[..end]);
        let off = read_u32(bytes, e + NAME_LEN)? as usize;
        let size = read_u32(bytes, e + NAME_LEN + 4)? as usize;
        let start = data_start + off;
        let slice = bytes.get(start..start + size).ok_or(KonkonError::Format {
            container: "arc",
            detail: format!("member '{name}' data out of range"),
        })?;
        members.push(Member {
            name: name.into_owned(),
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
        let (enc, _, _) = SHIFT_JIS.encode(&m.name);
        if enc.len() > NAME_LEN {
            return Err(KonkonError::Format {
                container: "arc",
                detail: format!("member name '{}' exceeds {NAME_LEN} bytes", m.name),
            });
        }
        let mut name_field = vec![0u8; NAME_LEN];
        name_field[..enc.len()].copy_from_slice(&enc);
        out.extend_from_slice(&name_field);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(m.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 0x18]);
        data.extend_from_slice(&m.data);
    }
    out.extend_from_slice(&data);
    Ok(out)
}
