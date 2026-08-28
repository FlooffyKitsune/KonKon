use konkon_core::engines::PatchFile;
use konkon_core::patch::apply::{apply, revert, status, PatchStatus};
use konkon_core::patch::build::write_artifact;
use konkon_core::project::Project;
use tempfile::tempdir;

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn apply_backs_up_then_revert_restores_byte_identical() {
    let home = tempdir().unwrap();
    let game = tempdir().unwrap();
    std::fs::create_dir_all(game.path().join("scene")).unwrap();
    std::fs::write(game.path().join("scene/op.bin"), b"ORIGINAL-JAPANESE").unwrap();

    let proj = Project::create(home.path(), "G").unwrap();
    let files = vec![PatchFile {
        rel_path: "scene/op.bin".into(),
        bytes: b"PATCHED-ENGLISH-LONGER".to_vec(),
    }];

    let manifest = write_artifact(&proj, game.path(), &files).unwrap();
    assert_eq!(manifest.entries[0].orig_sha256, sha(b"ORIGINAL-JAPANESE"));

    assert!(matches!(
        status(&proj, game.path()).unwrap(),
        PatchStatus::NotApplied
    ));
    apply(&proj, game.path()).unwrap();
    assert_eq!(
        std::fs::read(game.path().join("scene/op.bin")).unwrap(),
        b"PATCHED-ENGLISH-LONGER"
    );
    assert!(matches!(
        status(&proj, game.path()).unwrap(),
        PatchStatus::Applied
    ));
    assert_eq!(
        std::fs::read(proj.root.join("backups/scene/op.bin")).unwrap(),
        b"ORIGINAL-JAPANESE"
    );

    revert(&proj, game.path()).unwrap();
    assert_eq!(
        std::fs::read(game.path().join("scene/op.bin")).unwrap(),
        b"ORIGINAL-JAPANESE"
    );
    assert!(matches!(
        status(&proj, game.path()).unwrap(),
        PatchStatus::NotApplied
    ));
}

#[test]
fn slash_rel_path_works_against_a_canonicalized_game_dir() {
    let home = tempdir().unwrap();
    let game = tempdir().unwrap();
    // Canonicalize -> on Windows this yields a `\\?\` verbatim path where a raw
    // `.join("scene/op.bin")` would NOT split on the forward slash.
    let game_dir = game.path().canonicalize().unwrap();
    std::fs::create_dir_all(game_dir.join("scene")).unwrap();
    std::fs::write(game_dir.join("scene").join("op.bin"), b"ORIG").unwrap();

    let proj = Project::create(home.path(), "G").unwrap();
    let files = vec![PatchFile {
        rel_path: "scene/op.bin".into(),
        bytes: b"PATCHEDX".to_vec(),
    }];
    write_artifact(&proj, &game_dir, &files).unwrap();
    apply(&proj, &game_dir).unwrap();
    assert_eq!(
        std::fs::read(game_dir.join("scene").join("op.bin")).unwrap(),
        b"PATCHEDX"
    );
    assert!(matches!(
        status(&proj, &game_dir).unwrap(),
        PatchStatus::Applied
    ));
    revert(&proj, &game_dir).unwrap();
    assert_eq!(
        std::fs::read(game_dir.join("scene").join("op.bin")).unwrap(),
        b"ORIG"
    );
}

#[test]
fn revert_deletes_new_loose_files_that_had_no_original() {
    let home = tempdir().unwrap();
    let game = tempdir().unwrap();
    let proj = Project::create(home.path(), "G").unwrap();
    let files = vec![PatchFile {
        rel_path: "patch/extra.bin".into(),
        bytes: b"NEW".to_vec(),
    }];

    write_artifact(&proj, game.path(), &files).unwrap();
    apply(&proj, game.path()).unwrap();
    assert!(game.path().join("patch/extra.bin").exists());
    revert(&proj, game.path()).unwrap();
    assert!(!game.path().join("patch/extra.bin").exists());
}

#[test]
fn rebuild_after_apply_keeps_the_true_original_hash_via_the_backup() {
    let home = tempdir().unwrap();
    let game = tempdir().unwrap();
    std::fs::write(game.path().join("a.bin"), b"ORIGINAL").unwrap();
    let proj = Project::create(home.path(), "G").unwrap();

    let files = vec![PatchFile {
        rel_path: "a.bin".into(),
        bytes: b"PATCHED".to_vec(),
    }];
    let m1 = write_artifact(&proj, game.path(), &files).unwrap();
    assert_eq!(m1.entries[0].orig_sha256, sha(b"ORIGINAL"));
    apply(&proj, game.path()).unwrap();

    // The live file is now "PATCHED"; a naive re-read would poison the manifest.
    let m2 = write_artifact(&proj, game.path(), &files).unwrap();
    assert_eq!(
        m2.entries[0].orig_sha256,
        sha(b"ORIGINAL"),
        "rebuild must read orig hash from the backup, not the patched live file"
    );

    revert(&proj, game.path()).unwrap();
    assert_eq!(std::fs::read(game.path().join("a.bin")).unwrap(), b"ORIGINAL");
}

#[test]
fn revert_recovers_from_a_poisoned_manifest() {
    let home = tempdir().unwrap();
    let game = tempdir().unwrap();
    let proj = Project::create(home.path(), "G").unwrap();

    // Patched file is live; backup holds the true original; the manifest's
    // orig_sha256 was poisoned to equal the patched hash.
    std::fs::write(game.path().join("a.bin"), b"PATCHED").unwrap();
    std::fs::create_dir_all(proj.root.join("backups")).unwrap();
    std::fs::write(proj.root.join("backups/a.bin"), b"ORIGINAL").unwrap();
    std::fs::create_dir_all(proj.root.join("patch")).unwrap();
    let manifest = serde_json::json!({
        "entries": [{
            "rel_path": "a.bin",
            "orig_sha256": sha(b"PATCHED"),
            "patched_sha256": sha(b"PATCHED"),
        }]
    });
    std::fs::write(
        proj.root.join("patch/manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    revert(&proj, game.path()).unwrap();
    assert_eq!(
        std::fs::read(game.path().join("a.bin")).unwrap(),
        b"ORIGINAL",
        "file restored from backup despite the bad hash record"
    );
    assert!(
        !proj.root.join("backups/a.bin").exists(),
        "backup cleaned up so the project is not wedged"
    );
}

#[test]
fn apply_detects_tampered_patch_file() {
    let home = tempdir().unwrap();
    let game = tempdir().unwrap();
    std::fs::write(game.path().join("a.bin"), b"x").unwrap();
    let proj = Project::create(home.path(), "G").unwrap();
    write_artifact(
        &proj,
        game.path(),
        &[PatchFile {
            rel_path: "a.bin".into(),
            bytes: b"yy".to_vec(),
        }],
    )
    .unwrap();
    // Corrupt the staged patch file.
    std::fs::write(proj.root.join("patch/a.bin"), b"CORRUPT").unwrap();
    let err = apply(&proj, game.path()).unwrap_err();
    assert!(err.to_string().contains("sha256") || err.to_string().contains("mismatch"));
}
