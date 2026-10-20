mod support;
use support::report;

#[test]
fn file_mode_accepts_invalid_utf8_and_malformed_rust() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    let invalid = temp.path().join("invalid.rs");
    let mut source = b"fn invalid() {\n    let value = 1;\n}\n// ".to_vec();
    source.push(0xff);
    source.extend_from_slice(b"\n");
    std::fs::write(&invalid, source).expect("write invalid UTF-8 fixture");

    let malformed = temp.path().join("malformed.rs");
    std::fs::write(
        &malformed,
        b"fn broken( { let value = ; }\nfn recovered() { if true { let _ = 1; } }\n",
    )
    .expect("write malformed Rust fixture");
    for path in [invalid, malformed] {
        let result = report(&support::run("file", &path, None, None));
        let file = support::only_file(&result);
        assert_eq!(file["language"], "rust");
        assert!(file["nloc"].is_u64());
        assert!(file["functions"].is_array());
    }
}

#[test]
fn repo_skips_binary_files_without_a_registered_extension() {
    let temp = tempfile::tempdir().expect("create temporary directory");
    std::fs::write(temp.path().join("logo.png"), b"\x89PNG\r\n\x1a\n\xff\xfe")
        .expect("write binary fixture");
    std::fs::write(temp.path().join("lib.rs"), b"fn kept() {}\n").expect("write Rust fixture");

    let repo_report = report(&support::run("repo", temp.path(), None, None));
    let files = repo_report["files"].as_array().expect("files array");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["language"], "rust");
}

#[cfg(unix)]
#[test]
fn repo_follows_file_and_directory_symlinks_without_duplicate_reports() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("create temporary directory");
    let root = temp.path().join("repo");
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("create source directory");
    std::fs::write(
        root.join("Cargo.toml"),
        b"[package]\nname='links'\nversion='0.1.0'\n",
    )
    .expect("write manifest");
    std::fs::write(
        source_dir.join("lib.rs"),
        b"mod nested;\npub fn root_fn() { nested::leaf(); }\n",
    )
    .expect("write crate root");
    std::fs::write(source_dir.join("nested.rs"), b"pub fn leaf() {}\n").expect("write module file");
    symlink(source_dir.join("lib.rs"), root.join("lib-alias.rs")).expect("create file symlink");
    symlink(&source_dir, root.join("mirror")).expect("create directory symlink");

    let file_report = report(&support::run(
        "file",
        &root.join("lib-alias.rs"),
        None,
        None,
    ));
    assert_eq!(file_report["files"].as_array().unwrap().len(), 1);
    assert!(
        file_report["files"][0]["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("/repo/lib-alias.rs"))
    );

    let repo_report = report(&support::run("repo", &root, None, None));
    let files = repo_report["files"].as_array().expect("repo files");
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(
        |file| file["functions"].as_array().is_some_and(|functions| {
            functions
                .iter()
                .any(|function| function["name"] == "root_fn")
        })
    ));
    assert!(files.iter().any(|file| {
        file["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("nested.rs"))
    }));

    symlink(root.join("missing.rs"), root.join("broken.rs")).expect("create broken symlink");
    let broken_link_output = support::run("repo", &root, None, None);
    assert!(
        !broken_link_output.status.success(),
        "repository analysis should reject a broken symlink"
    );
}
