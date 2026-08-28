//! Top-level real-game acceptance test for the Ethornell / BGI round-trip.
//!
//! This exercises the whole public engine surface end to end against a real
//! install: `detect` -> `Engine::extract` -> `Engine::repack` (with every unit's
//! translation fed back as an identity copy of the Japanese) -> re-read every
//! rebuilt `.arc` and assert that each member which was parsed as a V1 script is
//! now byte-identical to the *decompressed* form of the original member, that
//! untouched members are preserved verbatim, and that member counts match.
//!
//! It is `#[ignore]`d and keyed on the `KONKON_TEST_GAME_DIR` environment
//! variable so a normal `cargo test` run skips it. Run it explicitly with:
//!
//! ```text
//! KONKON_TEST_GAME_DIR="C:\\Lump of Sugar\\若葉色のカルテット" \
//!   cargo test -p konkon-core --test verify_real_game -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};

use konkon_core::engines::ethornell::archive::read_arc;
use konkon_core::engines::ethornell::dsc::{decompress, is_dsc};
use konkon_core::engines::{detect, RepackCtx, RepackFile, RepackUnit};
use tempfile::tempdir;

/// The `<arc-stem>` half of an `ExtractedFile::rel_path` (`"data01000/00_x"` ->
/// `"data01000"`).
fn stem_of(rel_path: &str) -> &str {
    rel_path.split('/').next().unwrap_or(rel_path)
}

/// The `<member>` half of an `ExtractedFile::rel_path` (`"data01000/00_x"` ->
/// `"00_x"`).
fn member_of(rel_path: &str) -> &str {
    rel_path.split_once('/').map(|(_, m)| m).unwrap_or("")
}

#[test]
#[ignore = "needs a real Ethornell install via KONKON_TEST_GAME_DIR"]
fn every_scenario_script_roundtrips_byte_identical() {
    let game_dir = std::path::PathBuf::from(
        std::env::var("KONKON_TEST_GAME_DIR")
            .expect("set KONKON_TEST_GAME_DIR to the game install directory"),
    );

    // detect -> Ethornell
    let engine = detect(&game_dir).expect("an engine should be detected for the game dir");
    assert_eq!(engine.id(), "ethornell", "expected the Ethornell engine");

    // extract
    let work = tempdir().unwrap();
    let extraction = engine
        .extract(&game_dir, work.path())
        .expect("extract should succeed");
    assert!(
        !extraction.files.is_empty(),
        "no scenario scripts were extracted"
    );

    let total_files = extraction.files.len();
    let total_units: usize = extraction.files.iter().map(|f| f.units.len()).sum();

    // Which members were parsed as V1 scripts, grouped by arc stem.
    let mut script_members: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in &extraction.files {
        script_members
            .entry(stem_of(&f.rel_path).to_string())
            .or_default()
            .insert(member_of(&f.rel_path).to_string());
    }

    // Original arc contents, keyed by stem: (raw member bytes, decompressed body).
    struct OrigArc {
        raw: BTreeMap<String, Vec<u8>>,
        decompressed: BTreeMap<String, Vec<u8>>,
        member_count: usize,
    }
    let mut orig: BTreeMap<String, OrigArc> = BTreeMap::new();
    for stem in script_members.keys() {
        let bytes = std::fs::read(game_dir.join(format!("{stem}.arc")))
            .unwrap_or_else(|e| panic!("read {stem}.arc: {e}"));
        let members = read_arc(&bytes).unwrap_or_else(|e| panic!("read_arc({stem}.arc): {e:?}"));
        let mut raw = BTreeMap::new();
        let mut dec = BTreeMap::new();
        for m in &members {
            let body = if is_dsc(&m.data) {
                decompress(&m.data).unwrap_or_else(|_| m.data.clone())
            } else {
                m.data.clone()
            };
            raw.insert(m.name.clone(), m.data.clone());
            dec.insert(m.name.clone(), body);
        }
        orig.insert(
            stem.clone(),
            OrigArc {
                raw,
                decompressed: dec,
                member_count: members.len(),
            },
        );
    }

    // repack: feed every unit's jp_norm back as en_norm (identity rebuild).
    let repack_files: Vec<RepackFile> = extraction
        .files
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
        game_dir: &game_dir,
        work_dir: work.path(),
        files: repack_files,
    };
    let patches = engine.repack(&ctx).expect("repack should succeed");
    assert_eq!(
        patches.len(),
        script_members.len(),
        "one rebuilt .arc per touched archive stem"
    );

    let mut members_verified = 0usize;
    for patch in &patches {
        let stem = patch
            .rel_path
            .strip_suffix(".arc")
            .unwrap_or(&patch.rel_path)
            .to_string();
        let want_scripts = script_members
            .get(&stem)
            .unwrap_or_else(|| panic!("unexpected patch for {stem}"));
        let orig_arc = orig.get(&stem).expect("original arc cached");

        let rebuilt = read_arc(&patch.bytes)
            .unwrap_or_else(|e| panic!("read_arc(rebuilt {stem}.arc): {e:?}"));
        assert_eq!(
            rebuilt.len(),
            orig_arc.member_count,
            "{stem}.arc: member count changed"
        );

        for m in &rebuilt {
            if want_scripts.contains(&m.name) {
                let expected = orig_arc
                    .decompressed
                    .get(&m.name)
                    .expect("script member known in original");
                assert_eq!(
                    &m.data, expected,
                    "{stem}/{}: rebuilt script member is not byte-identical to the decompressed original",
                    m.name
                );
            } else {
                let expected = orig_arc.raw.get(&m.name).expect("member known in original");
                assert_eq!(
                    &m.data, expected,
                    "{stem}/{}: untouched member was modified",
                    m.name
                );
            }
            members_verified += 1;
        }
    }

    eprintln!(
        "verify_real_game: {total_files} script files across {} archive(s), {total_units} units, \
         {members_verified} archive members verified byte-identical",
        patches.len()
    );
}
