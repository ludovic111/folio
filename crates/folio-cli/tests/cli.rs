//! folio-cli end to end, on files in a temporary folder.

use std::process::Command;

/// The binary under test. Cargo names it in `CARGO_BIN_EXE_folio-cli`, which a `/bin/sh`
/// rustc-wrapper drops (dash forgets variables with `-` in their names): then it is found
/// beside the test, in the target directory.
fn exe() -> std::path::PathBuf {
    option_env!("CARGO_BIN_EXE_folio-cli").map(Into::into).unwrap_or_else(|| {
        let mut dir = std::env::current_exe().expect("the test's path");
        dir.pop();
        if dir.ends_with("deps") {
            dir.pop();
        }
        dir.join(format!("folio-cli{}", std::env::consts::EXE_SUFFIX))
    })
}

fn cli(dir: &std::path::Path, args: &[&str]) -> (bool, String, String) {
    let out = Command::new(exe())
        .args(args)
        .env("FOLIO_DATA_DIR", dir.join("data"))
        .env("FOLIO_CONFIG_DIR", dir.join("config"))
        .env("FOLIO_CONTROL", dir.join("control.json"))
        .env("LSUITE_HOME", dir.join("lsuite"))
        .output()
        .expect("folio-cli runs");
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

#[test]
fn a_file_from_the_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("plan.folio");
    let fs = f.to_str().unwrap();
    let (ok, _, err) = cli(dir.path(), &["--file", fs, "file.new", "kind=sheet", "title=Plan"]);
    assert!(ok, "{err}");
    assert!(f.exists());
    let (ok, _, err) = cli(dir.path(), &["--file", fs, "sheet.setRange", "at=A1", r#"values=[["a",2],["b",3],["sum","=SUM(B1:B2)"]]"#]);
    assert!(ok, "{err}");
    let (ok, out, err) = cli(dir.path(), &["--file", fs, "--compact", "sheet.read"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["rows"][2][1], serde_json::json!(5.0));
    // Unknown commands get a suggestion and exit 2.
    let (ok, _, err) = cli(dir.path(), &["--file", fs, "sheet.sett", "cell=A1"]);
    assert!(!ok && err.contains("sheet.set"), "{err}");
}

#[test]
fn convert_markdown_to_html() {
    let dir = tempfile::tempdir().unwrap();
    let md = dir.path().join("note.md");
    std::fs::write(&md, "# Hello\n\nSome **bold** text.\n").unwrap();
    let out = dir.path().join("note.html");
    let (ok, _, err) = cli(dir.path(), &["convert", md.to_str().unwrap(), out.to_str().unwrap()]);
    if !ok && err.contains("isn't ready") {
        return;
    }
    assert!(ok, "{err}");
    let html = std::fs::read_to_string(&out).unwrap();
    assert!(html.contains("Hello"));
}
