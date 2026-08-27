//! Tests for the Ethornell / BGI `BurikoCompiledScriptVer1.00` disassembler and
//! string extractor (`ethornell::script::parse`).
//!
//! The spec is VNTextPatch's `EthornellV1Disassembler` + `EthornellScript.Load`
//! / `GetStrings`; the fixtures below exercise the V1 opcode walk directly.

use konkon_core::engines::ethornell::archive::read_arc;
use konkon_core::engines::ethornell::dsc::{decompress, is_dsc};
use konkon_core::engines::ethornell::layout::{ScriptLayout, StrKind};
use konkon_core::engines::ethornell::script::parse;

const MAGIC: &[u8] = b"BurikoCompiledScriptVer1.00\0";

fn sjis(s: &str) -> Vec<u8> {
    let (enc, _, had_errors) = encoding_rs::SHIFT_JIS.encode(s);
    assert!(
        !had_errors,
        "fixture string not representable in CP932: {s:?}"
    );
    enc.into_owned()
}

fn push_i32(buf: &mut Vec<u8>, v: i32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// Push a `0x0003 push-string-address` instruction (`03 00 00 00 <addr>`).
fn push_str(buf: &mut Vec<u8>, addr: i32) {
    push_i32(buf, 0x0003);
    push_i32(buf, addr);
}

/// Minimal V1 script:
///
/// ```text
///   push_str NAME       ; "芳乃"
///   push_str MESSAGE    ; "「テスト」"
///   0x0140 show_message ; pops MESSAGE (top) -> Message, NAME -> CharacterName
///   push_str VOICE      ; "_PlayVoice"
///   0x001C user_fn_call ; pops VOICE -> Internal, reads fn name (not "_SelectEx")
///   0x001B              ; terminate (no unresolved code addresses)
/// ```
struct Fixture {
    bytes: Vec<u8>,
    code_offset: usize,
    code_length: usize,
    name: Vec<u8>,
    message: Vec<u8>,
    voice: Vec<u8>,
}

fn build_fixture() -> Fixture {
    let name = sjis("芳乃");
    let message = sjis("「テスト」");
    let voice = b"_PlayVoice".to_vec();

    // code: 2x push_str (8) + show_message (4) + push_str (8) + user_fn (4) + term (4)
    let code_length = 8 + 8 + 4 + 8 + 4 + 4;
    let code_offset = MAGIC.len() + 4; // header_size = 4 -> no header body
    let pool_start = code_offset + code_length;

    let addr_name = (pool_start - code_offset) as i32;
    let addr_message = (pool_start + name.len() + 1 - code_offset) as i32;
    let addr_voice = (pool_start + name.len() + 1 + message.len() + 1 - code_offset) as i32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    push_i32(&mut bytes, 4); // header_size

    assert_eq!(bytes.len(), code_offset);
    push_str(&mut bytes, addr_name);
    push_str(&mut bytes, addr_message);
    push_i32(&mut bytes, 0x0140);
    push_str(&mut bytes, addr_voice);
    push_i32(&mut bytes, 0x001C);
    push_i32(&mut bytes, 0x001B);
    assert_eq!(bytes.len(), pool_start);

    for s in [&name, &message, &voice] {
        bytes.extend_from_slice(s);
        bytes.push(0);
    }

    Fixture {
        bytes,
        code_offset,
        code_length,
        name,
        message,
        voice,
    }
}

#[test]
fn parses_minimal_v1_script() {
    let fx = build_fixture();
    let parsed = parse(&fx.bytes).expect("parse should succeed");
    let layout = &parsed.layout;

    assert_eq!(layout.code_offset, fx.code_offset);
    assert_eq!(layout.code_length, fx.code_length);
    assert_eq!(layout.prefix.len(), fx.code_offset + fx.code_length);
    assert_eq!(layout.prefix, &fx.bytes[..fx.code_offset + fx.code_length]);

    assert_eq!(layout.strings.len(), 3);

    assert_eq!(layout.strings[0].kind, StrKind::CharacterName);
    assert_eq!(layout.strings[0].bytes, fx.name);
    assert_eq!(
        layout.strings[0].text_offset,
        fx.code_offset + fx.code_length
    );
    // operand offset points at the i32 address inside the first push_str
    assert_eq!(layout.strings[0].operand_offset, fx.code_offset + 4);

    assert_eq!(layout.strings[1].kind, StrKind::Message);
    assert_eq!(layout.strings[1].bytes, fx.message);

    assert_eq!(layout.strings[2].kind, StrKind::Internal);
    assert_eq!(layout.strings[2].bytes, fx.voice);

    assert_eq!(layout.translatable_indices(), vec![0, 1]);
}

#[test]
fn layout_json_roundtrips() {
    let fx = build_fixture();
    let layout = parse(&fx.bytes).unwrap().layout;
    let json = layout.to_json().unwrap();
    let back = ScriptLayout::from_json(&json).unwrap();
    assert_eq!(back, layout);
}

#[test]
fn rejects_bad_magic() {
    let err = parse(b"not a buriko script at all, nope nope nope\0").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("script"), "got: {msg}");
}

#[test]
fn unknown_opcode_is_an_error_not_a_skip() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    push_i32(&mut bytes, 4);
    push_i32(&mut bytes, 0x0EEE); // not in the V1 operand-template map
    push_i32(&mut bytes, 0x001B);
    let err = parse(&bytes).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("script"), "got: {msg}");
    assert!(msg.to_lowercase().contains("opcode"), "got: {msg}");
}

#[test]
fn empty_string_at_message_address_is_internal() {
    // push_str "" ; push_str "" ; show_message -> both empty -> both Internal
    let code_length = 8 + 8 + 4 + 4;
    let code_offset = MAGIC.len() + 4;
    let pool_start = code_offset + code_length;
    let addr_empty = (pool_start - code_offset) as i32;

    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    push_i32(&mut bytes, 4);
    push_str(&mut bytes, addr_empty);
    push_str(&mut bytes, addr_empty);
    push_i32(&mut bytes, 0x0140);
    push_i32(&mut bytes, 0x001B);
    bytes.push(0); // the empty string

    let layout = parse(&bytes).unwrap().layout;
    assert_eq!(layout.strings.len(), 2);
    assert!(layout.strings.iter().all(|s| s.kind == StrKind::Internal));
    assert!(layout.translatable_indices().is_empty());
}

/// Gated real-file test. Requires `KONKON_TEST_GAME_DIR` pointing at the
/// 若葉色のカルテット install. Every `BurikoCompiledScript` DSC member of
/// `data01000.arc` must disassemble without error.
#[test]
#[ignore]
fn gated_parse_all_scripts_of_data01000() {
    let dir = std::env::var("KONKON_TEST_GAME_DIR")
        .expect("set KONKON_TEST_GAME_DIR to the game install directory");
    let arc_path = std::path::Path::new(&dir).join("data01000.arc");
    let bytes = std::fs::read(&arc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", arc_path.display()));
    let members = read_arc(&bytes).expect("read_arc(data01000.arc)");

    let mut scripts = 0usize;
    let mut parsed_ok = 0usize;
    let mut total_messages = 0usize;
    let mut total_names = 0usize;
    let mut total_internal = 0usize;
    let mut non_script = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for m in &members {
        if !is_dsc(&m.data) {
            continue;
        }
        let decompressed = match decompress(&m.data) {
            Ok(d) => d,
            Err(e) => {
                failures.push(format!("{}: decompress: {e}", m.name));
                continue;
            }
        };
        if !decompressed.starts_with(b"BurikoCompiledScriptVer1.00") {
            non_script += 1;
            continue;
        }
        scripts += 1;
        match parse(&decompressed) {
            Ok(p) => {
                parsed_ok += 1;
                for s in &p.layout.strings {
                    match s.kind {
                        StrKind::Message => total_messages += 1,
                        StrKind::CharacterName => total_names += 1,
                        StrKind::Internal => total_internal += 1,
                    }
                }
            }
            Err(e) => failures.push(format!("{}: parse: {e}", m.name)),
        }

        if m.name == "00com_12" {
            let layout = parse(&decompressed).unwrap().layout;
            eprintln!("--- 00com_12 first 8 Message strings ---");
            for s in layout
                .strings
                .iter()
                .filter(|s| s.kind == StrKind::Message)
                .take(8)
            {
                let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&s.bytes);
                eprintln!("  {text}");
            }
        }
    }

    eprintln!(
        "scripts: {scripts}, parsed_ok: {parsed_ok}, non_script_dsc: {non_script}, \
         messages: {total_messages}, names: {total_names}, internal: {total_internal}"
    );
    assert!(failures.is_empty(), "failures: {failures:#?}");
    assert!(
        parsed_ok >= 170,
        "expected >= 170 scripts parsed, got {parsed_ok}"
    );
    assert!(
        total_messages > 5000,
        "expected > 5000 Message strings, got {total_messages}"
    );
}
