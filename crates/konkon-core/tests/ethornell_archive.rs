use konkon_core::engines::ethornell::archive::{build_arc, read_arc, ARC_MAGIC};

/// Build a minimal valid BURIKO ARC20 blob with the given members.
fn synth_arc(members: &[(&str, &[u8])]) -> Vec<u8> {
    let count = members.len() as u32;
    let index_len = members.len() * 0x80;
    let mut out = Vec::new();
    out.extend_from_slice(ARC_MAGIC);
    out.extend_from_slice(&count.to_le_bytes());
    let mut data = Vec::new();
    for (name, bytes) in members {
        let mut name_field = vec![0u8; 0x60];
        let nb = name.as_bytes();
        name_field[..nb.len()].copy_from_slice(nb);
        out.extend_from_slice(&name_field);
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 0x18]);
        data.extend_from_slice(bytes);
    }
    assert_eq!(out.len(), 0x10 + index_len);
    out.extend_from_slice(&data);
    out
}

#[test]
fn read_arc_returns_members_in_order() {
    let blob = synth_arc(&[("scene/a.bin", b"AAA"), ("scene/b.bin", b"BBBB")]);
    let members = read_arc(&blob).unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].name, "scene/a.bin");
    assert_eq!(members[0].data, b"AAA");
    assert_eq!(members[1].data, b"BBBB");
}

#[test]
fn build_arc_roundtrips_byte_identical_when_unmodified() {
    let blob = synth_arc(&[("x", b"12345"), ("y", b"67")]);
    let members = read_arc(&blob).unwrap();
    let rebuilt = build_arc(&members).unwrap();
    assert_eq!(rebuilt, blob, "unmodified rebuild must be byte-identical");
}

#[test]
fn build_arc_updates_offsets_when_member_grows() {
    let blob = synth_arc(&[("x", b"12345"), ("y", b"67")]);
    let mut members = read_arc(&blob).unwrap();
    members[0].data = b"1234567890".to_vec();
    let rebuilt = build_arc(&members).unwrap();
    let reparsed = read_arc(&rebuilt).unwrap();
    assert_eq!(reparsed[0].data, b"1234567890");
    assert_eq!(reparsed[1].data, b"67");
}

#[test]
fn read_arc_rejects_bad_magic() {
    let err = read_arc(b"NOPE________\0\0\0\0").unwrap_err();
    assert!(err.to_string().contains("BURIKO") || err.to_string().contains("magic"));
}

#[test]
fn read_arc_rejects_absurd_entry_count_without_aborting() {
    // 76-byte archive whose header claims 0xFFFFFFFF entries. The old code did
    // `Vec::with_capacity(0xFFFFFFFF)` and aborted the process.
    let mut blob = Vec::new();
    blob.extend_from_slice(b"BURIKO ARC20");
    blob.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    blob.extend_from_slice(&[0u8; 60]); // pad to 76 bytes
    assert_eq!(blob.len(), 76);
    let err = read_arc(&blob).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("entries") || msg.contains("overflow"),
        "got: {msg}"
    );
}

#[test]
fn build_arc_preserves_reserved_bytes_and_exact_name_field() {
    // One entry with a multi-byte CP932 name and non-zero reserved bytes.
    let name = "配役/芳乃.bin";
    let (enc, _, had_err) = encoding_rs::SHIFT_JIS.encode(name);
    assert!(!had_err);
    let data: &[u8] = b"SOME BODY BYTES";

    let mut blob = Vec::new();
    blob.extend_from_slice(b"BURIKO ARC20");
    blob.extend_from_slice(&1u32.to_le_bytes());
    let mut name_field = vec![0u8; 0x60];
    name_field[..enc.len()].copy_from_slice(&enc);
    // trailing garbage after the NUL terminator, to prove it round-trips
    name_field[enc.len() + 1] = 0x7F;
    blob.extend_from_slice(&name_field);
    blob.extend_from_slice(&0u32.to_le_bytes()); // offset
    blob.extend_from_slice(&(data.len() as u32).to_le_bytes());
    let reserved: [u8; 0x18] = std::array::from_fn(|i| (i as u8) ^ 0xA5);
    blob.extend_from_slice(&reserved);
    blob.extend_from_slice(data);

    let members = read_arc(&blob).unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].reserved, reserved);
    assert_eq!(members[0].data, data);

    let rebuilt = build_arc(&members).unwrap();
    assert_eq!(rebuilt, blob, "byte-faithful round-trip");
}

#[test]
fn read_arc_rejects_truncated_index() {
    // header claims 4 members but the buffer is cut off inside the first entry
    let mut blob = Vec::new();
    blob.extend_from_slice(b"BURIKO ARC20");
    blob.extend_from_slice(&4u32.to_le_bytes());
    blob.extend_from_slice(&[0u8; 0x20]); // only part of one 0x80 entry
    let err = read_arc(&blob).unwrap_err();
    assert!(err.to_string().contains("truncat") || err.to_string().contains("arc"));
}
