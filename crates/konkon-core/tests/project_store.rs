use konkon_core::project::Project;
use tempfile::tempdir;

#[test]
fn create_project_lays_out_folder_and_db() {
    let tmp = tempdir().unwrap();
    let proj = Project::create(tmp.path(), "Senren Banka").unwrap();
    assert!(proj.root.ends_with("Senren Banka.konkon"));
    assert!(proj.root.join("project.db").is_file());
    assert!(proj.root.join("config.toml").is_file());
    for sub in ["extracted", "patch", "backups"] {
        assert!(proj.root.join(sub).is_dir(), "missing {sub}");
    }
    // reopen
    let reopened = Project::open(&proj.root).unwrap();
    assert_eq!(reopened.root, proj.root);
}

#[test]
fn create_refuses_to_clobber_an_existing_project() {
    let tmp = tempdir().unwrap();
    let first = Project::create(tmp.path(), "G").unwrap();
    // Put something in the db-adjacent tree to prove it is not wiped.
    std::fs::write(first.root.join("config.toml"), b"engine_id = 'ethornell'\n").unwrap();

    let err = match Project::create(tmp.path(), "G") {
        Ok(_) => panic!("expected create to fail on an existing project"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("already exists"), "got: {err}");
    assert_eq!(
        std::fs::read_to_string(first.root.join("config.toml")).unwrap(),
        "engine_id = 'ethornell'\n",
        "existing project left untouched"
    );
}

#[test]
fn transaction_rolls_back_on_error() {
    let tmp = tempdir().unwrap();
    let proj = Project::create(tmp.path(), "G").unwrap();
    let store = proj.store().unwrap();

    let r: konkon_core::Result<()> = store.transaction(|s| {
        s.insert_source_file("a.bin", "ethornell", "cp932", "x", "{}")?;
        Err(konkon_core::KonkonError::Other("boom".into()))
    });
    assert!(r.is_err());
    assert!(
        store.all_source_files().unwrap().is_empty(),
        "the failed transaction's insert must have rolled back"
    );

    store
        .transaction(|s| {
            s.insert_source_file("b.bin", "ethornell", "cp932", "y", "{}")?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.all_source_files().unwrap().len(), 1);
}

#[test]
fn store_roundtrips_source_file_units_translations() {
    let tmp = tempdir().unwrap();
    let proj = Project::create(tmp.path(), "G").unwrap();
    let store = proj.store().unwrap();

    let fid = store
        .insert_source_file(
            "scene/op.bin",
            "ethornell",
            "cp932",
            "deadbeef",
            r#"{"v":1}"#,
        )
        .unwrap();
    let uid = store
        .insert_unit(
            fid,
            0,
            "dialogue",
            "こんにちは{0}",
            "こんにちは{0}",
            r#"{"0":"\\n"}"#,
            Some("芳乃"),
        )
        .unwrap();
    store
        .upsert_translation(uid, "Hello{0}", "human", "human")
        .unwrap();
    store
        .upsert_translation(uid, "Hi there{0}", "human", "human")
        .unwrap(); // overwrite

    let units = store.units_for_file(fid).unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].speaker_raw.as_deref(), Some("芳乃"));
    assert_eq!(units[0].text_en.as_deref(), Some("Hi there{0}"));
    assert_eq!(units[0].status.as_deref(), Some("human"));

    let files = store.all_source_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].sha256, "deadbeef");
}
