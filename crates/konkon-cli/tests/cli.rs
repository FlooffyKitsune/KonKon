//! End-to-end tests for the `konkon` CLI binary.
//!
//! These drive the real binary via `assert_cmd` against a synthetic Ethornell
//! game directory (one `BURIKO ARC20` archive holding a minimal
//! `BurikoCompiledScriptVer1.00` script). The script fixture mirrors the one in
//! `konkon-core`'s `tests/ethornell_script.rs` — cross-crate test helpers can't
//! be shared, so it is reproduced here.

use assert_cmd::Command;
use konkon_core::engines::ethornell::archive::{build_arc, read_arc, Member};
use konkon_core::engines::ethornell::script::parse;
use predicates::str::contains;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Synthetic V1 script + archive fixtures
// ---------------------------------------------------------------------------

const MAGIC: &[u8] = b"BurikoCompiledScriptVer1.00\0";
const CODE_OFFSET: usize = 0x20;

fn sjis(s: &str) -> Vec<u8> {
    let (enc, _, had_errors) = encoding_rs::SHIFT_JIS.encode(s);
    assert!(!had_errors, "fixture string not CP932-representable: {s:?}");
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

/// Minimal V1 script: `push_str NAME; push_str MESSAGE; show_message; terminate`.
/// Produces two translatable units: ordinal 0 = CharacterName, 1 = Message.
fn synth_script(name: &str, message: &str) -> Vec<u8> {
    let name = sjis(name);
    let message = sjis(message);
    let pool_lens = [name.len(), message.len()];
    let code_length = 8 + 8 + 4 + 4;

    let mut code = Vec::new();
    push_str(&mut code, addr_of(code_length, &pool_lens, 0));
    push_str(&mut code, addr_of(code_length, &pool_lens, 1));
    push_i32(&mut code, 0x0140); // show_message
    push_i32(&mut code, 0x001B); // terminate
    assert_eq!(code.len(), code_length);

    let mut b = Vec::new();
    b.extend_from_slice(MAGIC);
    push_i32(&mut b, 4);
    assert_eq!(b.len(), CODE_OFFSET);
    b.extend_from_slice(&code);
    for s in [&name, &message] {
        b.extend_from_slice(s);
        b.push(0);
    }
    b
}

fn synth_arc(members: &[(&str, &[u8])]) -> Vec<u8> {
    let members: Vec<Member> = members
        .iter()
        .map(|(name, data)| Member::new(*name, data.to_vec()))
        .collect();
    build_arc(&members).expect("build_arc")
}

fn fake_game(dir: &std::path::Path, message: &str) {
    let script = synth_script("芳乃", message);
    let arc = synth_arc(&[("01_scene", &script)]);
    std::fs::write(dir.join("data01000.arc"), arc).unwrap();
}

fn konkon() -> Command {
    Command::cargo_bin("konkon").unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn full_pipeline_new_extract_export_import_build_apply_revert() {
    let workspace = tempdir().unwrap();
    let game = tempdir().unwrap();
    fake_game(game.path(), "おはよう");

    konkon()
        .args(["project", "new"])
        .arg(game.path())
        .args(["--name", "TestGame", "--into"])
        .arg(workspace.path())
        .assert()
        .success()
        .stdout(contains("ethornell"));

    let proj = workspace.path().join("TestGame.konkon");

    konkon().args(["extract"]).arg(&proj).assert().success();

    let export = workspace.path().join("lines.jsonl");
    konkon()
        .args(["export"])
        .arg(&proj)
        .args(["--out"])
        .arg(&export)
        .assert()
        .success();
    let exported = std::fs::read_to_string(&export).unwrap();
    assert!(
        exported.contains("\"jp\":\"おはよう\""),
        "export missing jp line: {exported}"
    );

    // Translate every line.
    let translated: String = exported
        .lines()
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["en"] = serde_json::Value::String("Good morning".into());
            v.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&export, translated).unwrap();

    konkon()
        .args(["import"])
        .arg(&proj)
        .arg(&export)
        .assert()
        .success();
    konkon()
        .args(["verify"])
        .arg(&proj)
        .assert()
        .success()
        .stdout(contains("OK"));
    konkon().args(["build"]).arg(&proj).assert().success();
    konkon()
        .args(["apply"])
        .arg(&proj)
        .assert()
        .success()
        .stdout(contains("Applied"));
    konkon()
        .args(["revert"])
        .arg(&proj)
        .assert()
        .success()
        .stdout(contains("NotApplied"));
}

/// The sequence C2 broke: after `apply`, a `build` must bail (not silently
/// record the patched files as originals), and `revert` + `verify` must then
/// both succeed.
#[test]
fn build_after_apply_bails_then_revert_and_verify_succeed() {
    let workspace = tempdir().unwrap();
    let game = tempdir().unwrap();
    fake_game(game.path(), "おはよう");

    konkon()
        .args(["project", "new"])
        .arg(game.path())
        .args(["--name", "Seq", "--into"])
        .arg(workspace.path())
        .assert()
        .success();
    let proj = workspace.path().join("Seq.konkon");

    konkon().args(["extract"]).arg(&proj).assert().success();

    let export = workspace.path().join("lines.jsonl");
    konkon()
        .args(["export"])
        .arg(&proj)
        .args(["--out"])
        .arg(&export)
        .assert()
        .success();
    // Translate exactly one line (ordinal 1, the message).
    let translated: String = std::fs::read_to_string(&export)
        .unwrap()
        .lines()
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            if v["ordinal"] == serde_json::json!(1) {
                v["en"] = serde_json::Value::String("Good morning".into());
            }
            v.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&export, translated).unwrap();
    konkon()
        .args(["import"])
        .arg(&proj)
        .arg(&export)
        .assert()
        .success();

    konkon().args(["build"]).arg(&proj).assert().success();
    konkon()
        .args(["apply"])
        .arg(&proj)
        .assert()
        .success()
        .stdout(contains("Applied"));

    // build now must bail with the applied-patch warning.
    konkon()
        .args(["build"])
        .arg(&proj)
        .assert()
        .failure()
        .stderr(contains("revert"));

    konkon()
        .args(["revert"])
        .arg(&proj)
        .assert()
        .success()
        .stdout(contains("NotApplied"));
    konkon()
        .args(["verify"])
        .arg(&proj)
        .assert()
        .success()
        .stdout(contains("OK"));
}

#[test]
fn project_new_on_unknown_dir_fails_cleanly() {
    let empty = tempdir().unwrap();
    let ws = tempdir().unwrap();
    konkon()
        .args(["project", "new"])
        .arg(empty.path())
        .args(["--into"])
        .arg(ws.path())
        .assert()
        .failure()
        .stderr(contains("no engine matched"));
}

/// Build-from-extract with no edits must yield a byte-identical patch member
/// (untranslated units fall back to `jp_norm`).
#[test]
fn build_without_edits_is_byte_identical() {
    let workspace = tempdir().unwrap();
    let game = tempdir().unwrap();
    fake_game(game.path(), "「テスト」");

    konkon()
        .args(["project", "new"])
        .arg(game.path())
        .args(["--name", "Wakaba", "--into"])
        .arg(workspace.path())
        .assert()
        .success();
    let proj = workspace.path().join("Wakaba.konkon");
    konkon().args(["extract"]).arg(&proj).assert().success();
    konkon().args(["build"]).arg(&proj).assert().success();

    let original = std::fs::read(game.path().join("data01000.arc")).unwrap();
    let patched = std::fs::read(proj.join("patch").join("data01000.arc")).unwrap();
    let orig_member = &read_arc(&original).unwrap()[0].data;
    let patched_member = &read_arc(&patched).unwrap()[0].data;
    assert_eq!(orig_member, patched_member, "no-op build must be identical");
}

/// The placeholder round-trip: a ruby-markup unit is translated with the
/// `{0}…{1}` placeholders kept, and the built patch must carry the English with
/// the ruby control codes restored, encoded as CP932, in the script's pool.
#[test]
fn placeholder_roundtrip_through_build() {
    let workspace = tempdir().unwrap();
    let game = tempdir().unwrap();
    // Message with a ruby span -> jp_norm == "{0}若葉{1}です".
    fake_game(game.path(), "<R わかば>若葉</R>です");

    konkon()
        .args(["project", "new"])
        .arg(game.path())
        .args(["--name", "Wakaba", "--into"])
        .arg(workspace.path())
        .assert()
        .success();
    let proj = workspace.path().join("Wakaba.konkon");
    konkon().args(["extract"]).arg(&proj).assert().success();

    let export = workspace.path().join("lines.jsonl");
    konkon()
        .args(["export"])
        .arg(&proj)
        .args(["--out"])
        .arg(&export)
        .assert()
        .success();
    let exported = std::fs::read_to_string(&export).unwrap();
    assert!(
        exported.contains("{0}") && exported.contains("{1}"),
        "ruby unit should normalize to placeholders: {exported}"
    );

    // Translate only the message unit (ordinal 1), keeping both placeholders.
    let translated: String = exported
        .lines()
        .map(|l| {
            let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
            if v["ordinal"] == serde_json::json!(1) {
                v["en"] = serde_json::Value::String("{0}Wakaba{1} here".into());
            }
            v.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&export, translated).unwrap();

    konkon()
        .args(["import"])
        .arg(&proj)
        .arg(&export)
        .assert()
        .success();
    konkon().args(["build"]).arg(&proj).assert().success();

    let patched = std::fs::read(proj.join("patch").join("data01000.arc")).unwrap();
    let members = read_arc(&patched).unwrap();
    let layout = parse(&members[0].data)
        .expect("patched member re-parses")
        .layout;
    let tix = layout.translatable_indices();
    let msg = &layout.strings[tix[1]].bytes;
    assert_eq!(
        msg,
        &sjis("<R わかば>Wakaba</R> here"),
        "English with restored ruby codes must be in the pool"
    );
}
