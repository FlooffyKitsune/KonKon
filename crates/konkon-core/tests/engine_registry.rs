use konkon_core::engines::{detect, EmitKind, Engine, Extraction, PatchFile, Probe, RepackCtx};
use std::path::Path;
use tempfile::tempdir;

struct FakeEngine;
impl Engine for FakeEngine {
    fn id(&self) -> &'static str {
        "fake"
    }
    fn display_name(&self) -> &'static str {
        "Fake"
    }
    fn probe(&self, dir: &Path) -> Probe {
        if dir.join("FAKE.marker").exists() {
            Probe::sure("marker")
        } else {
            Probe::none()
        }
    }
    fn extract(&self, _g: &Path, _w: &Path) -> konkon_core::Result<Extraction> {
        Ok(Extraction { files: vec![] })
    }
    fn emit(&self) -> EmitKind {
        EmitKind::NativeRepack
    }
    fn repack(&self, _c: &RepackCtx) -> konkon_core::Result<Vec<PatchFile>> {
        Ok(vec![])
    }
}

#[test]
fn detect_errors_when_nothing_matches() {
    let tmp = tempdir().unwrap();
    let err = detect(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("no engine matched"));
}

#[test]
fn probe_ordering_prefers_higher_confidence() {
    assert!(Probe::sure("x").confidence > Probe::maybe(0.4, "y").confidence);
    assert_eq!(Probe::none().confidence, 0.0);
}

#[test]
fn engine_trait_is_object_safe() {
    let fake: &dyn Engine = &FakeEngine;
    assert_eq!(fake.id(), "fake");
    assert_eq!(fake.display_name(), "Fake");
    assert_eq!(fake.probe(Path::new(".")).confidence, 0.0);
}
