use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

struct Server {
    child: Child,
    messages: Receiver<Value>,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    fn new(directory: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rumk"))
            .arg("server")
            .current_dir(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut input = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                if input.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let size: usize = line
                    .trim()
                    .strip_prefix("Content-Length: ")
                    .unwrap()
                    .parse()
                    .unwrap();
                line.clear();
                input.read_line(&mut line).unwrap();
                assert_eq!(line, "\r\n");
                let mut body = vec![0; size];
                input.read_exact(&mut body).unwrap();
                if tx.send(serde_json::from_slice(&body).unwrap()).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            child,
            messages: rx,
        };
        server.send(json!({"id":1,"method":"initialize","params":{"capabilities":{"workspace":{"workspaceEdit":{"documentChanges":true}},"textDocument":{"codeAction":{"codeActionLiteralSupport":{}}}}}}));
        let init = server.until(|m| m["id"] == 1);
        assert_eq!(init["result"]["capabilities"]["positionEncoding"], "utf-16");
        server.send(json!({"method":"initialized","params":{}}));
        server
    }
    fn send(&mut self, mut message: Value) {
        message["jsonrpc"] = json!("2.0");
        let bytes = serde_json::to_vec(&message).unwrap();
        let input = self.child.stdin.as_mut().unwrap();
        write!(input, "Content-Length: {}\r\n\r\n", bytes.len()).unwrap();
        input.write_all(&bytes).unwrap();
        input.flush().unwrap();
    }
    fn until(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        for _ in 0..100 {
            let message = self
                .messages
                .recv_timeout(Duration::from_secs(15))
                .expect("server response timed out");
            if predicate(&message) {
                return message;
            }
        }
        panic!("expected message was not received")
    }
    fn open(&mut self, uri: &str, text: &str, version: i64) {
        self.send(json!({"method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"makefile","version":version,"text":text}}}));
    }
    fn diagnostics(&self, uri: &str, version: i64) -> Value {
        self.until(|m| {
            m["method"] == "textDocument/publishDiagnostics"
                && m["params"]["uri"] == uri
                && m["params"]["version"] == version
        })["params"]["diagnostics"]
            .clone()
    }
    fn stop(&mut self) {
        self.send(json!({"id":99,"method":"shutdown"}));
        assert!(self.until(|m| m["id"] == 99)["result"].is_null());
        self.send(json!({"method":"exit"}));
        assert!(self.child.wait().unwrap().success());
    }
}
fn uri(path: &std::path::Path) -> String {
    let path = path.to_str().unwrap().replace('\\', "/");
    format!(
        "file://{}{}",
        if path.starts_with('/') { "" } else { "/" },
        path.replace(' ', "%20").replace('#', "%23")
    )
}

#[test]
fn server_reports_versioned_diagnostics_actions_and_incremental_updates() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".rumk.toml"),
        "[global]\nenable=['MK001','MK105']\n",
    )
    .unwrap();
    let path = dir.path().join("a #.mk");
    let uri = uri(&path);
    let mut server = Server::new(dir.path());
    let input = "# 😀\r\nall:\r\n    @echo hello\r\n";
    server.open(&uri, input, 1);
    let d = server.diagnostics(&uri, 1);
    assert_eq!(d[0]["code"], "MK001");
    assert_eq!(d[0]["range"]["start"]["line"], 2);
    server.send(json!({"id":2,"method":"textDocument/codeAction","params":{"textDocument":{"uri":uri},"range":{"start":{"line":2,"character":0},"end":{"line":2,"character":4}},"context":{"diagnostics":d,"only":["quickfix"]}}}));
    let actions = server.until(|m| m["id"] == 2)["result"].clone();
    assert_eq!(
        actions[0]["edit"]["documentChanges"][0]["textDocument"]["version"],
        1
    );
    assert_eq!(
        actions[0]["edit"]["documentChanges"][0]["edits"][0]["newText"],
        input.replace("    ", "\t")
    );
    server.send(json!({"method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"range":{"start":{"line":0,"character":2},"end":{"line":0,"character":4}},"text":"é"},{"range":{"start":{"line":2,"character":0},"end":{"line":2,"character":4}},"text":"\t"}]}}));
    assert_eq!(server.diagnostics(&uri, 2), json!([]));
    server.send(json!({"id":3,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":uri}}}));
    assert_eq!(server.until(|m| m["id"] == 3)["result"][0]["name"], "all");
    assert!(!path.exists());
    server.stop();
}

#[test]
fn unsaved_includes_invalidate_roots_and_closing_restores_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".rumk.toml"),
        "[global]\nenable=['MK206']\n",
    )
    .unwrap();
    let root = uri(&dir.path().join("Makefile"));
    let child = uri(&dir.path().join("new.mk"));
    let mut server = Server::new(dir.path());
    server.open(&root, "include new.mk\nall:;\n", 1);
    assert_eq!(server.diagnostics(&root, 1)[0]["code"], "MK206");
    server.open(&child, "X = yes\n", 1);
    assert_eq!(server.diagnostics(&root, 1), json!([]));
    server.send(json!({"method":"textDocument/didClose","params":{"textDocument":{"uri":child}}}));
    assert_eq!(server.diagnostics(&root, 1)[0]["code"], "MK206");
    assert!(!dir.path().join("new.mk").exists());
    server.stop();
}

#[test]
fn formatting_preserves_bom_and_utf16_ranges_and_reloads_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".rumk.toml");
    std::fs::write(&config, "[global]\nenable=['MK105']\n").unwrap();
    let uri = uri(&dir.path().join("Makefile"));
    let mut server = Server::new(dir.path());
    server.open(&uri, "\u{feff}É=😀\r\n", 1);
    server.diagnostics(&uri, 1);
    server.send(json!({"id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":true}}}));
    assert_eq!(
        server.until(|m| m["id"] == 2)["result"][0]["newText"],
        "\u{feff}É = 😀\r\n"
    );
    std::fs::write(&config, "[global]\ndisable=['MK105']\n").unwrap();
    server.send(json!({"method":"workspace/didChangeWatchedFiles","params":{"changes":[{"uri":uri,"type":2}]}}));
    assert_eq!(server.diagnostics(&uri, 1), json!([]));
    server.stop();
}

#[test]
fn invalid_configuration_is_isolated_to_its_project_and_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let good = directory.path().join("good");
    let bad = directory.path().join("bad");
    std::fs::create_dir(&good).unwrap();
    std::fs::create_dir(&bad).unwrap();
    std::fs::write(good.join("rumk.toml"), "[global]\nenable=['MK001']\n").unwrap();
    std::fs::write(bad.join("rumk.toml"), "[invalid TOML").unwrap();
    let good_uri = uri(&good.join("Makefile"));
    let bad_uri = uri(&bad.join("Makefile"));
    let mut server = Server::new(directory.path());
    server.open(&bad_uri, "all:;\n", 1);
    let diagnostics = server.diagnostics(&bad_uri, 1);
    assert_eq!(diagnostics[0]["code"], "configuration");
    server.open(&good_uri, "all:\n    echo hello\n", 1);
    let diagnostics = server.diagnostics(&good_uri, 1);
    assert_eq!(diagnostics[0]["code"], "MK001");
    std::fs::write(bad.join("rumk.toml"), "[global]\nenable=['MK001']\n").unwrap();
    server.send(json!({"method":"workspace/didChangeWatchedFiles","params":{"changes":[{"uri":uri(&bad.join("rumk.toml")),"type":2}]}}));
    assert_eq!(server.diagnostics(&bad_uri, 1), json!([]));
    server.stop();
}

#[cfg(unix)]
#[test]
fn unsaved_include_resolves_through_symlinked_workspace() {
    let directory = tempfile::tempdir().unwrap();
    let real = directory.path().join("real");
    let alias = directory.path().join("alias");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    std::fs::write(real.join("rumk.toml"), "[global]\nenable=['MK206']\n").unwrap();
    std::fs::write(real.join("Makefile"), "include tasks.mk\nall:;\n").unwrap();
    let root_uri = uri(&alias.join("Makefile"));
    let include_uri = uri(&alias.join("tasks.mk"));
    let mut server = Server::new(directory.path());
    server.open(&root_uri, "include tasks.mk\nall:;\n", 1);
    assert_eq!(server.diagnostics(&root_uri, 1)[0]["code"], "MK206");
    server.open(&include_uri, "# unsaved include\n", 1);
    assert_eq!(server.diagnostics(&root_uri, 1), json!([]));
    server.stop();
}
