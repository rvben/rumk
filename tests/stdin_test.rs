use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(root: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Invalid arguments may close the pipe before reading it.
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

#[test]
fn formatting_returns_only_the_buffer_and_never_writes_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a file.mk");
    std::fs::write(&path, "DISK=untouched\n").unwrap();
    for (input, expected) in [
        ("CC=cc\n", "CC = cc\n"),
        ("\u{feff}CC=cc\r\n", "\u{feff}CC = cc\r\n"),
        ("CC=cc", "CC = cc"),
        ("", ""),
        ("CC = cc\n", "CC = cc\n"),
        (
            "VALUE=text  # significant\n",
            "VALUE = text  # significant\n",
        ),
    ] {
        let args = [
            "fmt",
            "-",
            "--no-config",
            "--stdin-filename",
            "a file.mk",
            "--enable",
            "MK105",
        ];
        let output = run(root.path(), &args, input.as_bytes());
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(output.stdout, expected.as_bytes());
        assert!(output.stderr.is_empty());
        let repeat = run(root.path(), &args, &output.stdout);
        assert_eq!(repeat.stdout, output.stdout);
        assert!(repeat.status.success());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "DISK=untouched\n");
    }
}

#[test]
fn unsaved_root_uses_includes_and_reports_their_real_paths() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("project")).unwrap();
    std::fs::write(root.path().join("project/Makefile"), "DISK=untouched\n").unwrap();
    std::fs::write(
        root.path().join("project/tasks.mk"),
        ".IGNORE:\n.PHONY: test\ntest:;\n",
    )
    .unwrap();
    let output = run(
        root.path(),
        &[
            "check",
            "-",
            "--no-config",
            "--stdin-filename",
            "project/Makefile",
            "--enable",
            "MK214,MK215",
            "--output-format",
            "json",
        ],
        b"include tasks.mk\n.PHONY: all clean\nall clean:;\n",
    );
    assert_eq!(output.status.code(), Some(1));
    let diagnostics: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(diagnostics.as_array().unwrap().len(), 1, "{diagnostics}");
    assert_eq!(diagnostics[0]["rule"], "MK214");
    assert_eq!(
        diagnostics[0]["file"].as_str().unwrap().replace('\\', "/"),
        "project/tasks.mk"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("project/Makefile")).unwrap(),
        "DISK=untouched\n"
    );
}

#[test]
fn filename_selects_nearest_config_even_for_a_new_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    std::fs::write(
        root.path().join("nested/.rumk.toml"),
        "[global]\nenable = ['MK105']\n",
    )
    .unwrap();
    let output = run(
        root.path(),
        &["fmt", "-", "--stdin-filename", "nested/new.mk"],
        b"CC=cc\n",
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"CC = cc\n");
    assert!(!root.path().join("nested/new.mk").exists());
    let isolated = run(
        root.path(),
        &[
            "fmt",
            "-",
            "--stdin-filename",
            "nested/new.mk",
            "--no-config",
        ],
        b"CC=cc\n",
    );
    assert_eq!(isolated.stdout, b"CC=cc\n");
}

#[test]
fn stdin_modes_have_unambiguous_output_and_exit_status() {
    let root = tempfile::tempdir().unwrap();
    for (mode, expected) in [("--check", 1), ("--diff", 0)] {
        let output = run(
            root.path(),
            &["fmt", "-", mode, "--no-config", "--enable", "MK105"],
            b"CC=cc\n",
        );
        assert_eq!(output.status.code(), Some(expected));
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("+CC = cc"));
    }
    let clean = run(
        root.path(),
        &["fmt", "-", "--check", "--no-config", "--enable", "MK105"],
        b"CC = cc\n",
    );
    assert!(clean.status.success());
    assert!(clean.stdout.is_empty());
    let quiet = run(
        root.path(),
        &["check", "-", "--no-config", "--enable", "MK105", "--silent"],
        b"CC=cc\n",
    );
    assert_eq!(quiet.status.code(), Some(1));
    assert!(quiet.stdout.is_empty());
    let never = run(
        root.path(),
        &[
            "check",
            "-",
            "--no-config",
            "--enable",
            "MK105",
            "--fail-on",
            "never",
            "--output-format",
            "json",
        ],
        b"CC=cc\n",
    );
    assert!(never.status.success());
    let diagnostics: serde_json::Value = serde_json::from_slice(&never.stdout).unwrap();
    assert_eq!(diagnostics[0]["fix"]["applicability"], "safe");
    for args in [
        vec!["fmt", "-", "other.mk"],
        vec!["fmt", "--stdin-filename", "new.mk"],
        vec!["check", "-", "--fix"],
        vec!["fmt", "-", "--output-format", "json"],
        vec!["fmt", "-", "--stdin-filename", "."],
    ] {
        let output = run(root.path(), &args, b"CC=cc\n");
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    let invalid = run(root.path(), &["fmt", "-"], b"CC=\xff\n");
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
}

#[test]
fn comparison_corpus_has_identical_disk_and_buffer_diagnostics() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comparison");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(corpus.join("manifest.json")).unwrap()).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let root = tempfile::tempdir().unwrap();
        let fixture = corpus.join(case["name"].as_str().unwrap());
        for entry in std::fs::read_dir(fixture).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), root.path().join(entry.file_name())).unwrap();
        }
        std::fs::write(
            root.path().join(".rumk.toml"),
            format!(
                "[global]\nenable = {}\n[MK104]\nmax-lines = 2\n[MK215]\nrequired = ['test']\n",
                case["enable"]
            ),
        )
        .unwrap();
        let entry = case["entry"].as_str().unwrap();
        let input = std::fs::read(root.path().join(entry)).unwrap();
        let disk = run(
            root.path(),
            &["check", entry, "--output-format", "json"],
            b"",
        );
        let buffer = run(
            root.path(),
            &[
                "check",
                "-",
                "--stdin-filename",
                entry,
                "--output-format",
                "json",
            ],
            &input,
        );
        assert_eq!(buffer.status.code(), disk.status.code(), "{case}");
        assert_eq!(buffer.stdout, disk.stdout, "{case}");
        assert_eq!(buffer.stderr, disk.stderr, "{case}");
    }
}

#[test]
fn buffer_edits_use_original_byte_offsets_and_obey_suppressions() {
    let root = tempfile::tempdir().unwrap();
    let input = "\u{feff}CAFÉ=valeur\r\n";
    let output = run(
        root.path(),
        &[
            "check",
            "-",
            "--no-config",
            "--enable",
            "MK105",
            "--output-format",
            "json",
        ],
        input.as_bytes(),
    );
    let diagnostics: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let edit = &diagnostics[0]["fix"];
    let start = edit["range"]["start"].as_u64().unwrap() as usize;
    let end = edit["range"]["end"].as_u64().unwrap() as usize;
    let mut fixed = input.to_string();
    fixed.replace_range(start..end, edit["replacement"].as_str().unwrap());
    assert_eq!(fixed, "\u{feff}CAFÉ = valeur\r\n");
    let ignored = run(
        root.path(),
        &["fmt", "-", "--no-config", "--enable", "MK105"],
        b"# rumk-disable-next-line MK105\nCC=cc\n",
    );
    assert_eq!(ignored.stdout, b"# rumk-disable-next-line MK105\nCC=cc\n");
    let unfixable = run(
        root.path(),
        &[
            "fmt",
            "-",
            "--no-config",
            "--enable",
            "MK105",
            "--unfixable",
            "MK105",
        ],
        b"CC=cc\n",
    );
    assert_eq!(unfixable.stdout, b"CC=cc\n");
}
