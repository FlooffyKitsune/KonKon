//! Tests for `EthornellEngine` — the `Engine` trait impl wiring Ethornell/BGI
//! probe / extract / repack together (Task 9).
//!
//! The synthetic fixture is a copy of `ethornell_script.rs`'s minimal V1 script
//! (NOT DSC-wrapped, so `extract` parses it directly), packed into one
//! `BURIKO ARC20` archive via `archive::build_arc`.

use konkon_core::engines::ethornell::archive::{build_arc, read_arc, Member};
use konkon_core::engines::ethornell::script::parse;
use konkon_core::engines::ethornell::EthornellEngine;
use konkon_core::engines::{Engine, RepackCtx, RepackFile, RepackUnit};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Synthetic V1 script fixture (copied from tests/ethornell_script.rs)
// ---------------------------------------------------------------------------

const MAGIC: &[u8] = b"BurikoCompiledScriptVer1.00\0";
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

fn push_str(buf: &mut Vec<u8>, addr: i32) {
    push_i32(buf, 0x0003);
    push_i32(buf, addr);
}

fn addr_of(code_len: usize, pool_lens: &[usize], idx: usize) -> i32 {
    let mut a = code_len;
    for &l in &pool_lens[..idx] {
        a += l + 1;
    }
    a as i32
}

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

/// Minimal V1 script: `push_str NAME; push_str MESSAGE; show_message;
/// push_str VOICE; user_fn_call; terminate`.
/// strings: [CharacterName "芳乃", Message "「テスト」", Internal "_PlayVoice"].
fn build_script_bytes() -> Vec<u8> {
    let name = sjis("芳乃");
    let message = sjis("「テスト」");
    let voice = b"_PlayVoice".to_vec();
    let pool_lens = [name.len(), message.len(), voice.len()];
    let code_length = 8 + 8 + 4 + 8 + 4 + 4;

    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_length, &pool_lens, 0));
    push_str(&mut code, addr_of(code_length, &pool_lens, 1));
    push_i32(&mut code, 0x0140);
    push_str(&mut code, addr_of(code_length, &pool_lens, 2));
    push_i32(&mut code, 0x001C);
    push_i32(&mut code, 0x001B);
    assert_eq!(code.len(), code_length);

    assemble(&code, &[&name, &message, &voice])
}

const MEMBER_NAME: &str = "01_scene";
const ARC_STEM: &str = "data01";

/// Write `<dir>/data01.arc` holding one synthetic script member. Returns the
/// raw script bytes stored as that member.
fn setup_game_dir(dir: &std::path::Path) -> Vec<u8> {
    let script = build_script_bytes();
    let arc = build_arc(&[Member::new(MEMBER_NAME, script.clone())]).expect("build_arc");
    std::fs::write(dir.join(format!("{ARC_STEM}.arc")), &arc).expect("write arc");
    script
}

// ---------------------------------------------------------------------------
// probe
// ---------------------------------------------------------------------------

#[test]
fn probe_matches_a_buriko_arc20_archive() {
    let tmp = tempdir().unwrap();
    setup_game_dir(tmp.path());
    let p = EthornellEngine.probe(tmp.path());
    assert_eq!(p.confidence, 1.0, "note: {}", p.note);
}

#[test]
fn probe_matches_bgi_hvl_marker() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("BGI.hvl"), b"").unwrap();
    assert_eq!(EthornellEngine.probe(tmp.path()).confidence, 1.0);
}

#[test]
fn probe_partial_for_unrecognized_arc() {
    let tmp = tempdir().unwrap();
    std::fs::write(tmp.path().join("weird.arc"), b"NOT A BURIKO ARCHIVE").unwrap();
    assert_eq!(EthornellEngine.probe(tmp.path()).confidence, 0.6);
}

#[test]
fn probe_zero_for_empty_dir() {
    let tmp = tempdir().unwrap();
    assert_eq!(EthornellEngine.probe(tmp.path()).confidence, 0.0);
}

// ---------------------------------------------------------------------------
// extract
// ---------------------------------------------------------------------------

#[test]
fn extract_produces_one_file_with_translatable_units() {
    let tmp = tempdir().unwrap();
    setup_game_dir(tmp.path());
    let work = tempdir().unwrap();

    let ex = EthornellEngine
        .extract(tmp.path(), work.path())
        .expect("extract");
    assert_eq!(ex.files.len(), 1);
    let f = &ex.files[0];
    assert_eq!(f.rel_path, format!("{ARC_STEM}/{MEMBER_NAME}"));
    assert_eq!(f.encoding, "cp932");
    assert!(!f.sha256.is_empty());

    assert_eq!(f.units.len(), 2, "CharacterName + Message");
    assert_eq!(f.units[0].ordinal, 0);
    assert_eq!(f.units[0].jp_norm, "芳乃");
    assert_eq!(f.units[1].ordinal, 1);
    assert_eq!(f.units[1].jp_norm, "「テスト」");

    // layout_json round-trips through the parser's ScriptLayout.
    assert!(f.layout_json.contains("code_offset"));
}

#[test]
fn extract_normalizes_ruby_control_codes() {
    // A message with a ruby span -> two placeholders in jp_norm.
    let name = sjis("A");
    let message = sjis("<R かんじ>漢字</R>です");
    let pool_lens = [name.len(), message.len()];
    let code_length = 8 + 8 + 4 + 4;
    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_length, &pool_lens, 0));
    push_str(&mut code, addr_of(code_length, &pool_lens, 1));
    push_i32(&mut code, 0x0140);
    push_i32(&mut code, 0x001B);
    let script = assemble(&code, &[&name, &message]);

    let tmp = tempdir().unwrap();
    let arc = build_arc(&[Member::new(MEMBER_NAME, script)]).unwrap();
    std::fs::write(tmp.path().join(format!("{ARC_STEM}.arc")), arc).unwrap();

    let work = tempdir().unwrap();
    let ex = EthornellEngine.extract(tmp.path(), work.path()).unwrap();
    let msg_unit = &ex.files[0].units[1];
    assert_eq!(msg_unit.jp_norm, "{0}漢字{1}です");
    assert_eq!(msg_unit.placeholders.get("0"), Some("<R かんじ>"));
    assert_eq!(msg_unit.placeholders.get("1"), Some("</R>"));
}

// ---------------------------------------------------------------------------
// repack
// ---------------------------------------------------------------------------

fn repack_files_from_extract(
    game_dir: &std::path::Path,
    work: &std::path::Path,
    map_en: impl Fn(usize, &str) -> String,
) -> Vec<konkon_core::engines::PatchFile> {
    let ex = EthornellEngine.extract(game_dir, work).unwrap();
    let f = &ex.files[0];
    let units = f
        .units
        .iter()
        .map(|u| RepackUnit {
            ordinal: u.ordinal,
            jp_norm: u.jp_norm.clone(),
            en_norm: map_en(u.ordinal, &u.jp_norm),
            placeholder_map_json: u.placeholders.to_json(),
        })
        .collect();
    let ctx = RepackCtx {
        game_dir,
        work_dir: work,
        files: vec![RepackFile {
            rel_path: f.rel_path.clone(),
            layout_json: f.layout_json.clone(),
            units,
        }],
    };
    EthornellEngine.repack(&ctx).expect("repack")
}

#[test]
fn repack_noop_reproduces_the_original_member_bytes() {
    let tmp = tempdir().unwrap();
    let original = setup_game_dir(tmp.path());
    let work = tempdir().unwrap();

    let patches = repack_files_from_extract(tmp.path(), work.path(), |_, jp| jp.to_string());
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].rel_path, format!("{ARC_STEM}.arc"));

    let members = read_arc(&patches[0].bytes).expect("read rebuilt arc");
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].name, MEMBER_NAME);
    assert_eq!(
        members[0].data, original,
        "no-op repack must be byte-identical"
    );
}

#[test]
fn repack_with_english_lands_in_the_pool() {
    let tmp = tempdir().unwrap();
    setup_game_dir(tmp.path());
    let work = tempdir().unwrap();

    let patches = repack_files_from_extract(tmp.path(), work.path(), |ord, _| match ord {
        0 => "Yoshino".to_string(),
        _ => "\"Test\"".to_string(),
    });

    let members = read_arc(&patches[0].bytes).unwrap();
    let layout = parse(&members[0].data)
        .expect("rebuilt member re-parses")
        .layout;

    let tix = layout.translatable_indices();
    assert_eq!(layout.strings[tix[0]].bytes, b"Yoshino");
    assert_eq!(layout.strings[tix[1]].bytes, b"\"Test\"");
    // The Internal voice ref is untouched.
    let internal: Vec<_> = layout
        .strings
        .iter()
        .filter(|s| !s.kind.is_translatable())
        .collect();
    assert_eq!(internal.len(), 1);
    assert_eq!(internal[0].bytes, b"_PlayVoice");
}

#[test]
fn repack_unchanged_unit_is_left_as_original() {
    // en_norm == jp_norm for unit 0, English for unit 1.
    let tmp = tempdir().unwrap();
    setup_game_dir(tmp.path());
    let work = tempdir().unwrap();

    let patches = repack_files_from_extract(tmp.path(), work.path(), |ord, jp| match ord {
        0 => jp.to_string(),
        _ => "changed".to_string(),
    });
    let members = read_arc(&patches[0].bytes).unwrap();
    let layout = parse(&members[0].data).unwrap().layout;
    let tix = layout.translatable_indices();
    assert_eq!(layout.strings[tix[0]].bytes, sjis("芳乃"));
    assert_eq!(layout.strings[tix[1]].bytes, b"changed");
}

/// Choice screen where two translatable operands point at the same pool string
/// (`push_str C0; push_str C1; push_str C1; show_choice; terminate`). Ordinals 1
/// and 2 share one `pool_index`.
fn setup_shared_pool_game(dir: &std::path::Path) {
    let c0 = sjis("A");
    let c1 = sjis("B");
    let pool_lens = [c0.len(), c1.len()];
    let code_len = 8 * 3 + 4 + 4;
    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_len, &pool_lens, 0));
    push_str(&mut code, addr_of(code_len, &pool_lens, 1));
    push_str(&mut code, addr_of(code_len, &pool_lens, 1));
    push_i32(&mut code, 0x0160);
    push_i32(&mut code, 0x001B);
    let script = assemble(&code, &[&c0, &c1]);
    let arc = build_arc(&[Member::new(MEMBER_NAME, script)]).unwrap();
    std::fs::write(dir.join(format!("{ARC_STEM}.arc")), arc).unwrap();
}

#[test]
fn repack_rejects_conflicting_translations_for_a_shared_pool_string() {
    let tmp = tempdir().unwrap();
    setup_shared_pool_game(tmp.path());
    let work = tempdir().unwrap();

    let ex = EthornellEngine.extract(tmp.path(), work.path()).unwrap();
    let f = &ex.files[0];
    let units: Vec<RepackUnit> = f
        .units
        .iter()
        .map(|u| RepackUnit {
            ordinal: u.ordinal,
            jp_norm: u.jp_norm.clone(),
            en_norm: match u.ordinal {
                1 => "First".to_string(),
                2 => "Second".to_string(),
                _ => u.jp_norm.clone(),
            },
            placeholder_map_json: u.placeholders.to_json(),
        })
        .collect();
    let ctx = RepackCtx {
        game_dir: tmp.path(),
        work_dir: work.path(),
        files: vec![RepackFile {
            rel_path: f.rel_path.clone(),
            layout_json: f.layout_json.clone(),
            units,
        }],
    };
    let err = EthornellEngine.repack(&ctx).unwrap_err();
    assert!(
        err.to_string().contains("different translations"),
        "got: {err}"
    );
}

#[test]
fn repack_allows_identical_translations_for_a_shared_pool_string() {
    let tmp = tempdir().unwrap();
    setup_shared_pool_game(tmp.path());
    let work = tempdir().unwrap();

    let patches =
        repack_files_from_extract(tmp.path(), work.path(), |ord, jp| match ord {
            1 | 2 => "Same".to_string(),
            _ => jp.to_string(),
        });
    let members = read_arc(&patches[0].bytes).unwrap();
    let layout = parse(&members[0].data).unwrap().layout;
    let tix = layout.translatable_indices();
    assert_eq!(layout.strings[tix[1]].bytes, b"Same");
    assert_eq!(layout.strings[tix[2]].bytes, b"Same");
}

// ---------------------------------------------------------------------------
// Gated real-file round-trip (Step 5)
// ---------------------------------------------------------------------------

/// Requires `KONKON_TEST_GAME_DIR` pointing at the 若葉色のカルテット install.
#[test]
#[ignore]
fn gated_data01000_extract_repack_roundtrip() {
    use konkon_core::engines::ethornell::dsc::{decompress, is_dsc};

    let dir = std::env::var("KONKON_TEST_GAME_DIR")
        .expect("set KONKON_TEST_GAME_DIR to the game install directory");
    let game_dir = std::path::Path::new(&dir);
    let arc_path = game_dir.join("data01000.arc");
    let arc_bytes = std::fs::read(&arc_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", arc_path.display()));
    let original_members = read_arc(&arc_bytes).expect("read_arc(data01000.arc)");

    // Expected decompressed body for every member (scripts get DSC-expanded).
    let expected_decompressed: std::collections::BTreeMap<String, Vec<u8>> = original_members
        .iter()
        .map(|m| {
            let d = if is_dsc(&m.data) {
                decompress(&m.data).unwrap_or_else(|_| m.data.clone())
            } else {
                m.data.clone()
            };
            (m.name.clone(), d)
        })
        .collect();

    let work = tempdir().unwrap();
    let ex = EthornellEngine
        .extract(game_dir, work.path())
        .expect("extract");
    let data01_files: Vec<_> = ex
        .files
        .iter()
        .filter(|f| f.rel_path.starts_with("data01000/"))
        .collect();
    assert!(
        !data01_files.is_empty(),
        "no data01000 script files extracted"
    );

    let total_units: usize = data01_files.iter().map(|f| f.units.len()).sum();
    eprintln!(
        "data01000: {} script files, {} translation units",
        data01_files.len(),
        total_units
    );
    assert!(
        total_units > 5000,
        "expected > 5000 units, got {total_units}"
    );
    for f in &data01_files {
        for u in &f.units {
            assert!(!u.jp_norm.is_empty(), "empty unit in {}", f.rel_path);
        }
    }

    // Repack with en_norm := jp_norm for every unit.
    let repack_files: Vec<RepackFile> = data01_files
        .iter()
        .map(|f| RepackFile {
            rel_path: f.rel_path.clone(),
            layout_json: f.layout_json.clone(),
            units: f
                .units
                .iter()
                .map(|u| RepackUnit {
                    ordinal: u.ordinal,
                    jp_norm: u.jp_norm.clone(),
                    en_norm: u.jp_norm.clone(),
                    placeholder_map_json: u.placeholders.to_json(),
                })
                .collect(),
        })
        .collect();
    let ctx = RepackCtx {
        game_dir,
        work_dir: work.path(),
        files: repack_files,
    };
    let patches = EthornellEngine.repack(&ctx).expect("repack");
    assert_eq!(patches.len(), 1);
    assert_eq!(patches[0].rel_path, "data01000.arc");

    let rebuilt_members = read_arc(&patches[0].bytes).expect("read_arc(rebuilt)");
    assert_eq!(
        rebuilt_members.len(),
        original_members.len(),
        "member count preserved"
    );

    // Names of the members that `repack` actually rebuilt (the parseable V1
    // scripts). Those must be stored RAW == the original decompressed form.
    // Untouched members must keep their original (still DSC) bytes verbatim.
    let repacked: std::collections::BTreeSet<String> = data01_files
        .iter()
        .map(|f| f.rel_path["data01000/".len()..].to_string())
        .collect();
    let original_raw: std::collections::BTreeMap<String, Vec<u8>> = original_members
        .iter()
        .map(|m| (m.name.clone(), m.data.clone()))
        .collect();

    let mut rebuilt_checked = 0usize;
    let mut untouched_checked = 0usize;
    for m in &rebuilt_members {
        if repacked.contains(&m.name) {
            let expected = expected_decompressed
                .get(&m.name)
                .expect("member name known");
            assert_eq!(
                &m.data, expected,
                "repacked member {} raw body != original decompressed form",
                m.name
            );
            rebuilt_checked += 1;
        } else {
            let expected = original_raw.get(&m.name).expect("member name known");
            assert_eq!(
                &m.data, expected,
                "untouched member {} was modified",
                m.name
            );
            untouched_checked += 1;
        }
    }
    eprintln!(
        "round-trip: {rebuilt_checked} repacked members byte-identical to decompressed original, \
         {untouched_checked} untouched members preserved"
    );
    assert_eq!(rebuilt_checked, data01_files.len());
}
