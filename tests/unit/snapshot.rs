use super::Snapshot;

#[test]
fn cloned_snapshots_preserve_observed_bytes_and_stage_only_the_after_view() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("go.mod");
    std::fs::write(&path, b"module old").unwrap();
    let before = Snapshot::default();
    assert_eq!(&*before.read(&path).unwrap().unwrap(), b"module old");
    let after = before.clone();
    after
        .stage(Some(&path), Some(&path), b"module new")
        .unwrap();
    std::fs::write(&path, b"changed on disk").unwrap();
    assert_eq!(&*before.read(&path).unwrap().unwrap(), b"module old");
    assert_eq!(&*after.read(&path).unwrap().unwrap(), b"module new");
}

#[test]
fn missing_files_can_be_created_and_existing_files_deleted_virtually() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("go.mod");
    let before = Snapshot::default();
    assert!(before.read(&path).unwrap().is_none());
    let after = before.clone();
    after.stage(None, Some(&path), b"module created").unwrap();
    assert!(before.read(&path).unwrap().is_none());
    assert!(after.read(&path).unwrap().is_some());
    after.stage(Some(&path), None, b"").unwrap();
    assert!(after.read(&path).unwrap().is_none());
    assert!(!path.exists());
}
