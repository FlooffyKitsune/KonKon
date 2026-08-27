//! Tests for the Ethornell / BGI `BurikoCompiledScriptVer1.00` disassembler and
//! string extractor (`ethornell::script::parse`).
//!
//! The spec is VNTextPatch's `EthornellV1Disassembler` + `EthornellScript.Load`
//! / `GetStrings`; the fixtures below exercise the V1 opcode walk directly.

use konkon_core::engines::ethornell::archive::read_arc;
use konkon_core::engines::ethornell::dsc::{decompress, is_dsc};
use konkon_core::engines::ethornell::layout::{ScriptLayout, StrKind};
use konkon_core::engines::ethornell::script::parse;
use konkon_core::KonkonError;

const MAGIC: &[u8] = b"BurikoCompiledScriptVer1.00\0";
/// `magic.len() (0x1C) + header_size (4)` — every fixture uses `header_size = 4`
/// (no header body), so the bytecode starts here.
const CODE_OFFSET: usize = 0x20;

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

/// Address (relative to `code_offset`) of pool entry `idx`, given the bytecode
/// length and the NUL-excluded byte length of each pool string. The pool starts
/// immediately after the bytecode, i.e. at relative address `code_len`.
fn addr_of(code_len: usize, pool_lens: &[usize], idx: usize) -> i32 {
    let mut a = code_len;
    for &l in &pool_lens[..idx] {
        a += l + 1;
    }
    a as i32
}

/// Assemble `magic + header_size(4) + code + pool` (each pool string
/// NUL-terminated).
fn assemble(code: &[u8], pool: &[&[u8]]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(MAGIC);
    push_i32(&mut b, 4);
    assert_eq!(b.len(), CODE_OFFSET);
    b.extend_from_slice(code);
    for s in pool {
        b.extend_from_slice(s);
        b.push(0);
    }
    b
}

fn assert_script_format_err(err: KonkonError) {
    assert!(
        matches!(
            err,
            KonkonError::Format {
                container: "script",
                ..
            }
        ),
        "expected KonkonError::Format {{ container: \"script\", .. }}, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Minimal end-to-end fixture
// ---------------------------------------------------------------------------

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
    code_length: usize,
    name: Vec<u8>,
    message: Vec<u8>,
    voice: Vec<u8>,
}

fn build_fixture() -> Fixture {
    let name = sjis("芳乃");
    let message = sjis("「テスト」");
    let voice = b"_PlayVoice".to_vec();
    let pool_lens = [name.len(), message.len(), voice.len()];

    // 2x push_str (16) + show_message (4) + push_str (8) + user_fn (4) + term (4)
    let code_length = 8 + 8 + 4 + 8 + 4 + 4;

    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_length, &pool_lens, 0));
    push_str(&mut code, addr_of(code_length, &pool_lens, 1));
    push_i32(&mut code, 0x0140);
    push_str(&mut code, addr_of(code_length, &pool_lens, 2));
    push_i32(&mut code, 0x001C);
    push_i32(&mut code, 0x001B);
    assert_eq!(code.len(), code_length);

    let bytes = assemble(&code, &[&name, &message, &voice]);
    Fixture {
        bytes,
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

    assert_eq!(layout.code_offset, CODE_OFFSET);
    assert_eq!(layout.code_length, fx.code_length);
    assert_eq!(layout.prefix.len(), CODE_OFFSET + fx.code_length);
    assert_eq!(layout.prefix, &fx.bytes[..CODE_OFFSET + fx.code_length]);

    // Original pool, in file order.
    let pool_start = CODE_OFFSET + fx.code_length;
    assert_eq!(layout.pool.len(), 3);
    assert_eq!(layout.pool[0].offset, pool_start);
    assert_eq!(layout.pool[0].bytes, fx.name);
    assert_eq!(layout.pool[1].bytes, fx.message);
    assert_eq!(layout.pool[2].bytes, fx.voice);
    assert!(layout.pool_tail.is_empty());

    assert_eq!(layout.strings.len(), 3);

    assert_eq!(layout.strings[0].kind, StrKind::CharacterName);
    assert_eq!(layout.strings[0].bytes, fx.name);
    assert_eq!(layout.strings[0].pool_index, 0);
    assert_eq!(layout.strings[0].text_offset, pool_start);
    // operand offset points at the i32 address inside the first push_str
    assert_eq!(layout.strings[0].operand_offset, CODE_OFFSET + 4);

    assert_eq!(layout.strings[1].kind, StrKind::Message);
    assert_eq!(layout.strings[1].bytes, fx.message);
    assert_eq!(layout.strings[1].pool_index, 1);

    assert_eq!(layout.strings[2].kind, StrKind::Internal);
    assert_eq!(layout.strings[2].bytes, fx.voice);
    assert_eq!(layout.strings[2].pool_index, 2);

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

// ---------------------------------------------------------------------------
// Individual walk behaviors
// ---------------------------------------------------------------------------

#[test]
fn choice_screen_emits_every_operand_as_message_in_push_order() {
    let pool: [&[u8]; 3] = [b"AAA", b"BB", b"CCCC"];
    let pool_lens = [3, 2, 4];
    let code_len = 8 * 3 + 4 + 4;

    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_len, &pool_lens, 0));
    push_str(&mut code, addr_of(code_len, &pool_lens, 1));
    push_str(&mut code, addr_of(code_len, &pool_lens, 2));
    push_i32(&mut code, 0x0160); // show choice screen
    push_i32(&mut code, 0x001B); // terminate
    assert_eq!(code.len(), code_len);

    let layout = parse(&assemble(&code, &pool)).unwrap().layout;

    assert_eq!(layout.strings.len(), 3);
    assert!(layout.strings.iter().all(|s| s.kind == StrKind::Message));
    // Push order preserved (the C# restores order via Insert(0, ...)).
    assert_eq!(layout.strings[0].bytes, b"AAA");
    assert_eq!(layout.strings[1].bytes, b"BB");
    assert_eq!(layout.strings[2].bytes, b"CCCC");
    assert_eq!(
        layout
            .strings
            .iter()
            .map(|s| s.pool_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(layout.translatable_indices(), vec![0, 1, 2]);
}

#[test]
fn user_function_call_select_ex_runs_the_choice_handler() {
    // Choices pushed first, "_SelectEx" pushed last (it is the fn name, on top).
    let pool: [&[u8]; 3] = [b"cho0", b"cho1", b"_SelectEx"];
    let pool_lens = [4, 4, 9];
    let code_len = 8 * 3 + 4 + 4;

    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_len, &pool_lens, 0));
    push_str(&mut code, addr_of(code_len, &pool_lens, 1));
    push_str(&mut code, addr_of(code_len, &pool_lens, 2));
    push_i32(&mut code, 0x001C); // call user function
    push_i32(&mut code, 0x001B);
    assert_eq!(code.len(), code_len);

    let layout = parse(&assemble(&code, &pool)).unwrap().layout;

    assert_eq!(layout.strings.len(), 3);
    // fn name emitted first, as Internal
    assert_eq!(layout.strings[0].bytes, b"_SelectEx");
    assert_eq!(layout.strings[0].kind, StrKind::Internal);
    assert_eq!(layout.strings[0].pool_index, 2);
    // then the remaining stack, as Message, in push order
    assert_eq!(layout.strings[1].bytes, b"cho0");
    assert_eq!(layout.strings[1].kind, StrKind::Message);
    assert_eq!(layout.strings[2].bytes, b"cho1");
    assert_eq!(layout.strings[2].kind, StrKind::Message);
    assert_eq!(layout.translatable_indices(), vec![1, 2]);
}

#[test]
fn flush_opcodes_dump_the_stack_as_internal_and_the_walk_continues() {
    // (opcode, number of i32 operands it consumes per the V1 template map)
    for (opcode, operand_words) in [(0x007E_i32, 1_usize), (0x007F, 2), (0x00FE, 0)] {
        let pool: [&[u8]; 3] = [b"s0", b"s1", b"s2"];
        let pool_lens = [2, 2, 2];
        let code_len = 8 + 8 + 4 + 4 * operand_words + 8 + 4;

        let mut code = Vec::new();
        push_str(&mut code, addr_of(code_len, &pool_lens, 0));
        push_str(&mut code, addr_of(code_len, &pool_lens, 1));
        push_i32(&mut code, opcode);
        for _ in 0..operand_words {
            push_i32(&mut code, 0);
        }
        push_str(&mut code, addr_of(code_len, &pool_lens, 2));
        push_i32(&mut code, 0x001B);
        assert_eq!(code.len(), code_len, "opcode 0x{opcode:04X}");

        let layout = parse(&assemble(&code, &pool)).unwrap().layout;

        // s1, s0 flushed at the opcode (reverse pop order); s2 flushed at 0x001B.
        assert_eq!(layout.strings.len(), 3, "opcode 0x{opcode:04X}");
        assert!(
            layout.strings.iter().all(|s| s.kind == StrKind::Internal),
            "opcode 0x{opcode:04X}"
        );
        assert_eq!(layout.strings[0].bytes, b"s1", "opcode 0x{opcode:04X}");
        assert_eq!(layout.strings[1].bytes, b"s0", "opcode 0x{opcode:04X}");
        assert_eq!(layout.strings[2].bytes, b"s2", "opcode 0x{opcode:04X}");
        assert!(layout.translatable_indices().is_empty());
    }
}

#[test]
fn termination_is_deferred_until_all_forward_code_addresses_are_resolved() {
    // 0x0001 (template "c") pushes a forward code address A = 20.
    //   pos after           = code_offset + 8
    // 0x001B  (first term)   consumed = 12  -> 20 < 12 is false -> DO NOT break
    // 0x0003 push_str S0
    // 0x001C  user fn call   pops S0 -> Internal, reads "_Foo" (not _SelectEx)
    // 0x001B  (second term)  consumed = 28  -> 20 < 28 is true  -> break
    let pool: [&[u8]; 1] = [b"_Foo"];
    let pool_lens = [4];
    let code_len = 4 + 4 + 4 + 8 + 4 + 4; // = 28

    let mut code = Vec::new();
    push_i32(&mut code, 0x0001);
    push_i32(&mut code, 20); // forward code address
    push_i32(&mut code, 0x001B); // must NOT terminate here
    push_str(&mut code, addr_of(code_len, &pool_lens, 0));
    push_i32(&mut code, 0x001C);
    push_i32(&mut code, 0x001B); // terminates here
    assert_eq!(code.len(), code_len);

    let layout = parse(&assemble(&code, &pool)).unwrap().layout;

    // Stopped at the SECOND terminator, not the first (12).
    assert_eq!(layout.code_length, 28);
    // The push_str + user-fn AFTER the first terminator were still walked.
    assert_eq!(layout.strings.len(), 1);
    assert_eq!(layout.strings[0].kind, StrKind::Internal);
    assert_eq!(layout.strings[0].bytes, b"_Foo");
}

// ---------------------------------------------------------------------------
// Malformed input -> Err (never panic). The brief's explicit contract.
// ---------------------------------------------------------------------------

#[test]
fn rejects_bad_magic() {
    let err = parse(b"not a buriko script at all, nope nope nope\0").unwrap_err();
    assert!(err.to_string().contains("magic"), "got: {err}");
    assert_script_format_err(err);
}

#[test]
fn rejects_negative_header_size() {
    let mut b = MAGIC.to_vec();
    push_i32(&mut b, -4);
    assert_script_format_err(parse(&b).unwrap_err());
}

#[test]
fn rejects_code_offset_past_eof() {
    let mut b = MAGIC.to_vec();
    push_i32(&mut b, 0x0100); // header_size 256, file is 32 bytes
    assert_script_format_err(parse(&b).unwrap_err());
}

#[test]
fn rejects_truncated_opcode_mid_stream() {
    let mut b = MAGIC.to_vec();
    push_i32(&mut b, 4);
    b.extend_from_slice(&[0x03, 0x00]); // 2 bytes: cannot read an i32 opcode
    assert_script_format_err(parse(&b).unwrap_err());
}

#[test]
fn rejects_truncated_operand_mid_stream() {
    let mut b = MAGIC.to_vec();
    push_i32(&mut b, 4);
    push_i32(&mut b, 0x0003); // push-string-address opcode
    b.extend_from_slice(&[0x00, 0x00]); // partial operand
    assert_script_format_err(parse(&b).unwrap_err());
}

#[test]
fn rejects_unknown_opcode_in_a_table_gap() {
    // 0x0154 sits in a real gap (table has 0x0153 then 0x0156) — an unknown
    // opcode must be an error, not a silent skip.
    let mut code = Vec::new();
    push_i32(&mut code, 0x0154);
    push_i32(&mut code, 0x001B);
    let err = parse(&assemble(&code, &[])).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("opcode"),
        "got: {err}"
    );
    assert_script_format_err(err);
}

#[test]
fn rejects_negative_string_address() {
    let mut code = Vec::new();
    push_str(&mut code, -100); // code_offset + (-100) < 0
    push_i32(&mut code, 0x001C);
    push_i32(&mut code, 0x001B);
    assert_script_format_err(parse(&assemble(&code, &[b"x"])).unwrap_err());
}

#[test]
fn rejects_string_address_past_end() {
    let mut code = Vec::new();
    push_str(&mut code, 100_000);
    push_i32(&mut code, 0x001C);
    push_i32(&mut code, 0x001B);
    assert_script_format_err(parse(&assemble(&code, &[b"x"])).unwrap_err());
}

#[test]
fn rejects_unterminated_pool_string() {
    let mut code = Vec::new();
    push_str(&mut code, 16); // addr 16 == code_len -> start of the pool region
    push_i32(&mut code, 0x001C);
    push_i32(&mut code, 0x001B);
    assert_eq!(code.len(), 16);

    let mut bytes = MAGIC.to_vec();
    push_i32(&mut bytes, 4);
    bytes.extend_from_slice(&code);
    bytes.extend_from_slice(b"_Foo"); // no NUL, runs off the end

    assert_script_format_err(parse(&bytes).unwrap_err());
}

#[test]
fn rejects_string_reference_not_on_a_pool_boundary() {
    // push_str NAME, push_str MSG, show_message; MSG's address points one byte
    // into the pool region (mid-string), not at a NUL-split boundary.
    let name = b"X";
    let msg = b"AB";
    let pool_lens = [name.len(), msg.len()];
    let code_len = 8 + 8 + 4 + 4;

    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_len, &pool_lens, 0));
    push_str(&mut code, addr_of(code_len, &pool_lens, 0) + 1); // mid-string
    push_i32(&mut code, 0x0140);
    push_i32(&mut code, 0x001B);

    assert_script_format_err(parse(&assemble(&code, &[name, msg])).unwrap_err());
}

#[test]
fn empty_string_at_message_address_is_internal() {
    // push_str "" ; push_str "" ; show_message -> both empty -> both Internal
    let code_length = 8 + 8 + 4 + 4;
    let addr_empty = addr_of(code_length, &[0], 0);

    let mut code = Vec::new();
    push_str(&mut code, addr_empty);
    push_str(&mut code, addr_empty);
    push_i32(&mut code, 0x0140);
    push_i32(&mut code, 0x001B);

    let layout = parse(&assemble(&code, &[b""])).unwrap().layout;
    assert_eq!(layout.pool.len(), 1);
    assert!(layout.pool[0].bytes.is_empty());
    assert_eq!(layout.strings.len(), 2);
    assert!(layout.strings.iter().all(|s| s.kind == StrKind::Internal));
    assert!(layout.strings.iter().all(|s| s.pool_index == 0));
    assert!(layout.translatable_indices().is_empty());
}

// ---------------------------------------------------------------------------
// Gated real-file test
// ---------------------------------------------------------------------------

/// Requires `KONKON_TEST_GAME_DIR` pointing at the 若葉色のカルテット install.
/// Every `BurikoCompiledScript` DSC member of `data01000.arc` must disassemble
/// without error, and its recorded pool must reconstruct the original region
/// byte-for-byte (the property Task 8's identity gate needs).
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
        let layout = match parse(&decompressed) {
            Ok(p) => p.layout,
            Err(e) => {
                failures.push(format!("{}: parse: {e}", m.name));
                continue;
            }
        };
        parsed_ok += 1;

        for s in &layout.strings {
            match s.kind {
                StrKind::Message => total_messages += 1,
                StrKind::CharacterName => total_names += 1,
                StrKind::Internal => total_internal += 1,
            }
        }

        // Pool invariants (Task 8 identity-gate prerequisites).
        if layout.pool.is_empty() {
            failures.push(format!("{}: empty pool", m.name));
        }
        for s in &layout.strings {
            let entry = &layout.pool[s.pool_index];
            if s.text_offset != entry.offset || s.bytes != entry.bytes {
                failures.push(format!(
                    "{}: string ref does not match its pool entry",
                    m.name
                ));
                break;
            }
        }
        let pool_start = layout.code_offset + layout.code_length;
        let mut rebuilt = Vec::new();
        for e in &layout.pool {
            rebuilt.extend_from_slice(&e.bytes);
            rebuilt.push(0);
        }
        rebuilt.extend_from_slice(&layout.pool_tail);
        if rebuilt != decompressed[pool_start..] {
            failures.push(format!(
                "{}: pool does not reconstruct the original region",
                m.name
            ));
        }

        if m.name == "00com_12" {
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
