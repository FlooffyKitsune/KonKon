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
