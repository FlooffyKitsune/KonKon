//! Tests for the Ethornell / BGI `DSC FORMAT 1.00` decompressor.

use konkon_core::engines::ethornell::archive::read_arc;
use konkon_core::engines::ethornell::dsc::{decompress, is_dsc, MAGIC};

/// Faithful mirror of the reference `hash_update` key-stream step, used only to
/// obfuscate the hand-built fixture's code-length table.
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

/// Build an all-literal DSC blob.
///
/// Every one of the 512 symbols is assigned a 9-bit code, so the canonical
/// Huffman tree is a perfect depth-9 binary tree whose leaf for symbol `s` is
/// reached by the 9-bit big-endian value `s`. A literal byte `b` is therefore
/// emitted as the 9 bits `0b0_bbbb_bbbb`. This exercises the table de-obfuscation,
/// the canonical-tree builder, and the MSB-first bit reader all at once.
fn build_all_literal_dsc(payload: &[u8], key: u32) -> Vec<u8> {
    let mut blob = Vec::new();
    blob.extend_from_slice(MAGIC); // 0x00: 16-byte signature
    blob.extend_from_slice(&key.to_le_bytes()); // 0x10: hash key
    blob.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // 0x14: decompressed size
    blob.extend_from_slice(&(payload.len() as u32).to_le_bytes()); // 0x18: symbol count (unused)
    blob.extend_from_slice(&0u32.to_le_bytes()); // 0x1C: padding

    // 0x20: 512-byte obfuscated code-length table; raw[i] = 9 + keystream_byte(i)
    let mut hash = key;
    for _ in 0..512 {
        let k = (hash_update(&mut hash) & 0xFF) as u8;
        blob.push(9u8.wrapping_add(k));
    }

    // 0x220: bitstream, 9 bits per payload byte, MSB-first.
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    for &b in payload {
        acc = (acc << 9) | u32::from(b); // symbol == byte value, top (9th) bit is 0
        nbits += 9;
        while nbits >= 8 {
            nbits -= 8;
            blob.push(((acc >> nbits) & 0xFF) as u8);
        }
    }
    if nbits > 0 {
        blob.push(((acc << (8 - nbits)) & 0xFF) as u8);
    }
    blob
}

#[test]
fn decompress_all_literal_roundtrip() {
    let payload = b"BurikoCompiledScriptVer1.00\0test";
    let blob = build_all_literal_dsc(payload, 0x1234_5678);
    let out = decompress(&blob).expect("decompress should succeed");
    assert_eq!(out.as_slice(), payload.as_slice());
}

#[test]
fn decompress_all_literal_roundtrip_zero_key() {
    let payload = b"the quick brown fox jumps over the lazy dog 0123456789";
    let blob = build_all_literal_dsc(payload, 0);
    let out = decompress(&blob).expect("decompress should succeed");
    assert_eq!(out.as_slice(), payload.as_slice());
}

#[test]
fn is_dsc_accepts_signature_and_rejects_others() {
    let mut good = MAGIC.to_vec();
    good.extend_from_slice(&[0u8; 32]);
    assert!(is_dsc(&good));

    // The 16th byte is not part of the checked prefix.
    let mut lenient = b"DSC FORMAT 1.00X".to_vec();
    lenient.extend_from_slice(&[0u8; 32]);
    assert!(is_dsc(&lenient));

    assert!(!is_dsc(b"BurikoCompiledScriptVer1.00"));
    assert!(!is_dsc(b"DSC FORMAT 2.00\0"));
    assert!(!is_dsc(b"DSC"));
    assert!(!is_dsc(b""));
}

#[test]
fn decompress_rejects_short_header() {
    let err = decompress(b"DSC FORMAT 1.00\0\x01\x02\x03\x04").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("dsc"), "got: {msg}");
    assert!(
        msg.to_lowercase().contains("truncat") || msg.contains("header"),
        "got: {msg}"
    );
}

#[test]
fn decompress_rejects_bad_magic() {
    let mut blob = vec![0u8; 0x240];
    blob[..15].copy_from_slice(b"NOT A DSC BLOB!");
    let err = decompress(&blob).unwrap_err();
    assert!(err.to_string().contains("dsc"));
}

/// Gated real-file test. Requires `KONKON_TEST_GAME_DIR` pointing at the game
/// install (e.g. `C:\Lump of Sugar\若葉色のカルテット`). Every DSC member of
/// `data01000.arc` must decompress to exactly its declared size.
#[test]
#[ignore]
fn gated_decompress_all_dsc_members_of_data01000() {
    let dir = std::env::var("KONKON_TEST_GAME_DIR")
        .expect("set KONKON_TEST_GAME_DIR to the game install directory");
    let arc_path = std::path::Path::new(&dir).join("data01000.arc");
    let bytes = std::fs::read(&arc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", arc_path.display()));
    let members = read_arc(&bytes).expect("read_arc(data01000.arc)");

    let mut dsc_members = 0usize;
    let mut passed = 0usize;
    let mut buriko = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for m in &members {
        if !is_dsc(&m.data) {
            continue;
        }
        dsc_members += 1;
        let declared = u32::from_le_bytes(m.data[0x14..0x18].try_into().unwrap()) as usize;
        match decompress(&m.data) {
            Ok(out) if out.len() == declared => {
                passed += 1;
                if out.starts_with(b"BurikoCompiledScriptVer1.00") {
                    buriko += 1;
                }
            }
            Ok(out) => {
                failures.push(format!(
                    "{}: decoded {} bytes, header declared {declared}",
                    m.name,
                    out.len()
                ));
            }
            Err(e) => failures.push(format!("{}: {e}", m.name)),
        }
    }

    eprintln!("DSC members: {dsc_members}, passed: {passed}, BurikoCompiledScript: {buriko}");
    assert!(failures.is_empty(), "failures: {failures:#?}");
    assert!(
        passed >= 170,
        "expected >= 170 DSC members to pass, got {passed}"
    );
    assert!(
        buriko >= 1,
        "expected at least one BurikoCompiledScriptVer1.00 payload"
    );
}
