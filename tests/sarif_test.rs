use serde_json::Value;
use std::path::Path;
use std::process::Command;

#[test]
fn sarif_cli_matches_json_on_the_comparison_corpus() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/comparison");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(corpus.join("manifest.json")).unwrap()).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        let temp = tempfile::tempdir().unwrap();
        for entry in std::fs::read_dir(corpus.join(case["name"].as_str().unwrap())).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), temp.path().join(entry.file_name())).unwrap();
        }
        std::fs::write(
            temp.path().join(".rumk.toml"),
            format!(
                "[global]\nenable = {}\n[MK104]\nmax-lines = 2\n[MK215]\nrequired = ['test']\n",
                case["enable"]
            ),
        )
        .unwrap();
        let run = |format: &str| {
            Command::new(env!("CARGO_BIN_EXE_rumk"))
                .current_dir(temp.path())
                .args([
                    "check",
                    case["entry"].as_str().unwrap(),
                    "--output-format",
                    format,
                ])
                .output()
                .unwrap()
        };
        let json = run("json");
        let sarif = run("sarif");
        assert_eq!(sarif.status.code(), json.status.code(), "{case}");
        assert!(sarif.stderr.is_empty());
        assert_eq!(sarif.stdout, run("sarif").stdout);
        let json: Value = serde_json::from_slice(&json.stdout).unwrap();
        let sarif: Value = serde_json::from_slice(&sarif.stdout).unwrap();
        let results = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), json.as_array().unwrap().len(), "{case}");
        for (result, diagnostic) in results.iter().zip(json.as_array().unwrap()) {
            assert_eq!(result["ruleId"], diagnostic["rule"]);
            assert_eq!(result["message"]["text"], diagnostic["message"]);
            let location = &result["locations"][0]["physicalLocation"];
            assert_eq!(location["artifactLocation"]["uri"], diagnostic["file"]);
            assert_eq!(location["region"]["startLine"], diagnostic["line"]);
            assert_eq!(location["region"]["startColumn"], diagnostic["column"]);
        }
    }
}

#[test]
fn sarif_supports_unsaved_buffers_and_special_filenames() {
    use std::io::Write;
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rumk"))
        .current_dir(temp.path())
        .args([
            "check",
            "-",
            "--no-config",
            "--stdin-filename",
            "a #é.mk",
            "--enable",
            "MK105",
            "--output-format",
            "sarif",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"CC=cc\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let result = &value["runs"][0]["results"][0];
    assert_eq!(
        result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "a%20%23%C3%A9.mk"
    );
    assert_eq!(result["properties"]["fixApplicability"], "safe");
    assert!(!temp.path().join("a #é.mk").exists());
}
