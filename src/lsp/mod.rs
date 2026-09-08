//! Native stdio Language Server Protocol 3.17 support, using the lint engine.
mod analysis;
mod text;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Condvar, Mutex,
};

pub(super) const MAX_BUFFER: usize = 8 * 1024 * 1024;
const MAX_TOTAL: usize = 32 * 1024 * 1024;

#[derive(Clone)]
struct Document {
    uri: String,
    path: PathBuf,
    text: Arc<str>,
    version: i64,
}
#[derive(Clone)]
struct Snapshot {
    documents: BTreeMap<String, Document>,
    config: Option<PathBuf>,
    no_config: bool,
    cancelled: Arc<AtomicBool>,
    versioned_actions: bool,
}
struct Job {
    generation: u64,
    snapshot: Snapshot,
    request: Option<Value>,
}
struct Finished {
    generation: u64,
    request: Option<Value>,
    result: Result<Value>,
}
enum Event {
    Message(Value),
    Finished(Finished),
    End,
    Error(String),
}
#[derive(Default)]
struct Queue {
    diagnostics: Option<Job>,
    requests: VecDeque<Job>,
    closed: bool,
}
type Work = Arc<(Mutex<Queue>, Condvar)>;

fn error(id: Value, code: i32, message: impl AsRef<str>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.as_ref()}})
}
fn reply(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","method":method,"params":params})
}
fn send(out: &mut impl Write, message: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(message)?;
    write!(out, "Content-Length: {}\r\n\r\n", bytes.len())?;
    out.write_all(&bytes)?;
    out.flush()?;
    Ok(())
}

fn read_message(input: &mut impl BufRead) -> Result<Option<Value>> {
    let mut size = None;
    let mut headers = 0;
    loop {
        let mut line = String::new();
        // Bound individual headers before allocation, including clients that
        // omit a newline. Body lengths are bytes, never Unicode characters.
        let read = input.take(8193).read_line(&mut line)?;
        if read == 0 {
            if headers == 0 {
                return Ok(None);
            }
            bail!("Truncated LSP headers");
        }
        headers += read;
        if headers > 8192 {
            bail!("LSP headers exceed 8 KiB");
        }
        if !line.is_ascii() || !line.ends_with("\r\n") {
            bail!("LSP headers must be ASCII with CRLF endings");
        }
        if line == "\r\n" {
            break;
        }
        let (name, value) = line
            .trim_end()
            .split_once(':')
            .context("Invalid LSP header")?;
        if name.eq_ignore_ascii_case("Content-Type") {
            for parameter in value.split(';').skip(1) {
                if let Some((key, encoding)) = parameter.trim().split_once('=') {
                    if key.trim().eq_ignore_ascii_case("charset")
                        && !["utf-8", "utf8"]
                            .iter()
                            .any(|v| encoding.trim().eq_ignore_ascii_case(v))
                    {
                        bail!("Only UTF-8 LSP content is supported");
                    }
                }
            }
        }
        if name.eq_ignore_ascii_case("Content-Length") {
            if size.is_some() {
                bail!("Duplicate Content-Length");
            }
            size = Some(value.trim().parse::<usize>()?);
        }
    }
    let size = size.context("Missing Content-Length")?;
    if size > MAX_BUFFER {
        bail!("LSP message exceeds 8 MiB");
    }
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

fn worker(work: Work, events: mpsc::SyncSender<Event>) {
    let mut cache: Option<(u64, analysis::Reports)> = None;
    loop {
        let job = {
            let (lock, wake) = &*work;
            let mut queue = lock.lock().unwrap();
            while queue.requests.is_empty() && queue.diagnostics.is_none() && !queue.closed {
                queue = wake.wait(queue).unwrap();
            }
            if queue.closed {
                return;
            }
            queue
                .requests
                .pop_front()
                .or_else(|| queue.diagnostics.take())
                .unwrap()
        };
        if job.snapshot.cancelled.load(Ordering::Relaxed) {
            continue;
        }
        let reports = if cache
            .as_ref()
            .is_some_and(|(generation, _)| *generation == job.generation)
        {
            Ok(())
        } else {
            analysis::analyze(&job.snapshot).map(|reports| {
                cache = Some((job.generation, reports));
            })
        };
        if job.snapshot.cancelled.load(Ordering::Relaxed) {
            cache = None;
            continue;
        }
        let result = reports.and_then(|()| {
            let reports = &cache.as_ref().unwrap().1;
            if let Some(request) = &job.request {
                analysis::request(&job.snapshot, request["method"].as_str().unwrap_or(""), &request["params"], reports)
            } else {
                let mut notifications = Vec::new();
                for (path,report) in reports {
                    let doc = job.snapshot.documents.values().find(|doc| &doc.path==path);
                    let uri = doc.map(|doc| doc.uri.clone()).unwrap_or_else(|| text::uri(path));
                    let mut params = json!({"uri":uri,"diagnostics":report.diagnostics.iter().map(|d| analysis::diagnostic(&report.text,d)).collect::<Vec<_>>()});
                    if let Some(doc) = doc { params["version"] = json!(doc.version); }
                    notifications.push(notification("textDocument/publishDiagnostics",params));
                }
                Ok(json!(notifications))
            }
        });
        let finished = Finished {
            generation: job.generation,
            request: job.request,
            result,
        };
        if events.send(Event::Finished(finished)).is_err() {
            return;
        }
    }
}

/// Run until shutdown/exit or EOF. Stdout is exclusively framed JSON-RPC.
pub fn serve(config: Option<PathBuf>, no_config: bool) -> Result<u8> {
    if no_config && config.is_some() {
        bail!("--config cannot be combined with --no-config");
    }
    // Backpressure also bounds queued protocol bodies, not just parsed buffers.
    let (events, rx) = mpsc::sync_channel(16);
    let input_events = events.clone();
    std::thread::spawn(move || {
        let mut input = BufReader::new(std::io::stdin());
        loop {
            match read_message(&mut input) {
                Ok(Some(message)) => {
                    if input_events.send(Event::Message(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = input_events.send(Event::End);
                    break;
                }
                Err(e) => {
                    let _ = input_events.send(Event::Error(e.to_string()));
                    break;
                }
            }
        }
    });
    let work: Work = Arc::default();
    let worker_work = work.clone();
    std::thread::spawn(move || worker(worker_work, events));
    let result = session(&rx, &work, config, no_config, &mut std::io::stdout().lock());
    work.0.lock().unwrap().closed = true;
    work.1.notify_one();
    result
}

fn session(
    rx: &mpsc::Receiver<Event>,
    work: &Work,
    config: Option<PathBuf>,
    no_config: bool,
    out: &mut impl Write,
) -> Result<u8> {
    let mut snapshot = Snapshot {
        documents: BTreeMap::new(),
        config,
        no_config,
        cancelled: Arc::default(),
        versioned_actions: false,
    };
    let mut initialized = false;
    let mut shutdown = false;
    let mut watch = false;
    let mut generation = 0;
    let mut published = BTreeSet::new();
    let mut pending: BTreeMap<String, (Value, Arc<AtomicBool>)> = BTreeMap::new();
    while let Ok(event) = rx.recv() {
        match event {
            Event::End => return Ok(if shutdown { 0 } else { 1 }),
            Event::Error(e) => {
                send(out, &error(Value::Null, -32700, e))?;
                return Ok(1);
            }
            Event::Finished(finished) => {
                if let Some(request) = finished.request {
                    let id = request["id"].clone();
                    if pending.remove(&id.to_string()).is_none() {
                        continue;
                    }
                    let message = if finished.generation != generation || shutdown {
                        reply(id, json!([]))
                    } else {
                        match finished.result {
                            Ok(result) => reply(id, result),
                            Err(e) => error(id, -32603, e.to_string()),
                        }
                    };
                    send(out, &message)?;
                } else if finished.generation == generation && !shutdown {
                    match finished.result {
                        Ok(messages) => {
                            let messages =
                                messages.as_array().context("Invalid worker response")?;
                            let current: BTreeSet<_> = messages
                                .iter()
                                .filter_map(|m| m["params"]["uri"].as_str().map(str::to_string))
                                .collect();
                            for uri in published.difference(&current) {
                                send(
                                    out,
                                    &notification(
                                        "textDocument/publishDiagnostics",
                                        json!({"uri":uri,"diagnostics":[]}),
                                    ),
                                )?;
                            }
                            for message in messages {
                                send(out, message)?;
                            }
                            published = current;
                        }
                        Err(e) => {
                            for uri in &published {
                                send(
                                    out,
                                    &notification(
                                        "textDocument/publishDiagnostics",
                                        json!({"uri":uri,"diagnostics":[]}),
                                    ),
                                )?;
                            }
                            published.clear();
                            send(
                                out,
                                &notification(
                                    "window/showMessage",
                                    json!({"type":1,"message":format!("Rumk analysis failed: {e}")}),
                                ),
                            )?;
                        }
                    }
                }
            }
            Event::Message(message) => {
                if !message.is_object() || message["jsonrpc"] != "2.0" {
                    send(
                        out,
                        &error(Value::Null, -32600, "Expected a JSON-RPC 2.0 object"),
                    )?;
                    continue;
                }
                let Some(method) = message["method"].as_str() else {
                    continue;
                };
                let id = message.get("id").cloned();
                let params = &message["params"];
                if method == "exit" {
                    return Ok(if shutdown { 0 } else { 1 });
                }
                if method == "initialize" && !initialized {
                    initialized = true;
                    snapshot.versioned_actions = params["capabilities"]["workspace"]
                        ["workspaceEdit"]["documentChanges"]
                        == true
                        && params["capabilities"]["textDocument"]["codeAction"]
                            ["codeActionLiteralSupport"]
                            .is_object();
                    watch = params["capabilities"]["workspace"]["didChangeWatchedFiles"]
                        ["dynamicRegistration"]
                        == true;
                    if let Some(id) = id {
                        send(
                            out,
                            &reply(
                                id,
                                json!({"capabilities":{
                        "positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":2,"save":true},
                        "documentFormattingProvider":true,"documentSymbolProvider":true,
                        "codeActionProvider":if snapshot.versioned_actions {json!({"codeActionKinds":["quickfix","source.fixAll.rumk"]})} else {json!(false)}},
                        "serverInfo":{"name":"rumk","version":env!("CARGO_PKG_VERSION")}}),
                            ),
                        )?;
                    }
                    continue;
                }
                if !initialized || shutdown {
                    if let Some(id) = id {
                        send(
                            out,
                            &error(
                                id,
                                if shutdown { -32600 } else { -32002 },
                                "Server is not running",
                            ),
                        )?;
                    }
                    continue;
                }
                match method {
                    "initialized" => {
                        if watch {
                            send(
                                out,
                                &json!({"jsonrpc":"2.0","id":"rumk/watch","method":"client/registerCapability","params":{"registrations":[{"id":"rumk/files","method":"workspace/didChangeWatchedFiles","registerOptions":{"watchers":[{"globPattern":"**/*"}]}}]}}),
                            )?;
                        }
                    }
                    "shutdown" => {
                        shutdown = true;
                        snapshot.cancelled.store(true, Ordering::Relaxed);
                        for (_, (id, cancel)) in std::mem::take(&mut pending) {
                            cancel.store(true, Ordering::Relaxed);
                            send(out, &error(id, -32800, "Server shutting down"))?;
                        }
                        if let Some(id) = id {
                            send(out, &reply(id, Value::Null))?;
                        }
                    }
                    "$/cancelRequest" => {
                        if let Some((id, cancel)) = pending.remove(&params["id"].to_string()) {
                            cancel.store(true, Ordering::Relaxed);
                            work.0
                                .lock()
                                .unwrap()
                                .requests
                                .retain(|job| !job.snapshot.cancelled.load(Ordering::Relaxed));
                            send(out, &error(id, -32800, "Request cancelled"))?;
                        }
                    }
                    "textDocument/didOpen"
                    | "textDocument/didChange"
                    | "textDocument/didClose"
                    | "textDocument/didSave"
                    | "workspace/didChangeWatchedFiles"
                    | "workspace/didChangeConfiguration" => {
                        match update(&mut snapshot, method, params) {
                            Ok(false) => continue,
                            Err(e) => {
                                send(
                                    out,
                                    &notification(
                                        "window/showMessage",
                                        json!({"type":1,"message":e.to_string()}),
                                    ),
                                )?;
                                continue;
                            }
                            Ok(true) => {}
                        }
                        snapshot.cancelled.store(true, Ordering::Relaxed);
                        snapshot.cancelled = Arc::default();
                        generation += 1;
                        for (_, (id, cancel)) in std::mem::take(&mut pending) {
                            cancel.store(true, Ordering::Relaxed);
                            send(out, &error(id, -32801, "Document content changed"))?;
                        }
                        let mut queue = work.0.lock().unwrap();
                        queue.requests.clear();
                        queue.diagnostics = Some(Job {
                            generation,
                            snapshot: snapshot.clone(),
                            request: None,
                        });
                        work.1.notify_one();
                    }
                    "textDocument/codeAction"
                    | "textDocument/formatting"
                    | "textDocument/documentSymbol" => {
                        let Some(id) = id else {
                            continue;
                        };
                        if method == "textDocument/codeAction" && !snapshot.versioned_actions {
                            send(out, &reply(id, json!([])))?;
                            continue;
                        }
                        if pending.len() >= 64 || pending.contains_key(&id.to_string()) {
                            send(
                                out,
                                &error(id, -32600, "Too many requests or duplicate request ID"),
                            )?;
                            continue;
                        }
                        let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                        if !snapshot.documents.contains_key(uri) {
                            send(out, &error(id, -32602, "Document is not open"))?;
                            continue;
                        }
                        if method == "textDocument/codeAction" {
                            if let Err(e) =
                                analysis::selection(&snapshot.documents[uri].text, params)
                            {
                                send(out, &error(id, -32602, e.to_string()))?;
                                continue;
                            }
                        }
                        let mut task_snapshot = snapshot.clone();
                        task_snapshot.cancelled = Arc::default();
                        pending.insert(id.to_string(), (id, task_snapshot.cancelled.clone()));
                        work.0.lock().unwrap().requests.push_back(Job {
                            generation,
                            snapshot: task_snapshot,
                            request: Some(message.clone()),
                        });
                        work.1.notify_one();
                    }
                    _ => {
                        if let Some(id) = id {
                            send(out, &error(id, -32601, "Method not supported"))?;
                        }
                    }
                }
            }
        }
    }
    Ok(1)
}

fn update(snapshot: &mut Snapshot, method: &str, params: &Value) -> Result<bool> {
    if method.starts_with("workspace/") || method == "textDocument/didSave" {
        return Ok(true);
    }
    let uri = params["textDocument"]["uri"]
        .as_str()
        .context("Missing document URI")?;
    if method == "textDocument/didClose" {
        return Ok(snapshot.documents.remove(uri).is_some());
    }
    let version = params["textDocument"]["version"]
        .as_i64()
        .context("Missing document version")?;
    if let Some(old) = snapshot.documents.get(uri) {
        if version <= old.version {
            return Ok(false);
        }
    }
    let doc = if method == "textDocument/didOpen" {
        let text = params["textDocument"]["text"]
            .as_str()
            .context("Missing document text")?;
        Document {
            uri: uri.into(),
            path: text::path(uri)?,
            text: text.into(),
            version,
        }
    } else {
        let old = snapshot
            .documents
            .get(uri)
            .context("Document is not open")?;
        let changes = params["contentChanges"]
            .as_array()
            .context("Missing changes")?;
        Document {
            text: text::change(&old.text, changes)?.into(),
            version,
            ..old.clone()
        }
    };
    let total: usize = snapshot
        .documents
        .values()
        .filter(|d| d.uri != uri)
        .map(|d| d.text.len())
        .sum();
    if doc.text.len() > MAX_BUFFER
        || total + doc.text.len() > MAX_TOTAL
        || (!snapshot.documents.contains_key(uri) && snapshot.documents.len() >= 128)
    {
        bail!("Open document size limit exceeded");
    }
    if snapshot
        .documents
        .values()
        .any(|d| d.uri != uri && d.path == doc.path)
    {
        bail!("Document is already open through another URI");
    }
    snapshot.documents.insert(uri.into(), doc);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_rejects_duplicate_lengths_oversized_messages_and_truncation() {
        for frame in [
            "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
            "Content-Length: 999999999\r\n\r\n",
            "Content-Length: 3\r\n\r\n{}",
            "Other: x\r\n\r\n{}",
            "Content-Length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=utf-16\r\n\r\n{}",
            "Content-Length: 2\n\n{}",
        ] {
            assert!(read_message(&mut std::io::Cursor::new(frame)).is_err());
        }
        let mut wire = Vec::new();
        send(&mut wire, &json!({"text":"😀"})).unwrap();
        assert_eq!(
            read_message(&mut std::io::Cursor::new(wire))
                .unwrap()
                .unwrap()["text"],
            "😀"
        );
    }
    #[test]
    fn initialization_capabilities_and_stale_diagnostics_are_respected() {
        let (tx, rx) = mpsc::channel();
        for message in [
            json!({"id":0,"method":"textDocument/formatting"}),
            json!({"id":1,"method":"initialize","params":{"capabilities":{}}}),
            json!({"id":2,"method":"textDocument/codeAction","params":{}}),
        ] {
            let mut message = message;
            message["jsonrpc"] = json!("2.0");
            tx.send(Event::Message(message)).unwrap();
        }
        tx.send(Event::Finished(Finished {
            generation: 99,
            request: None,
            result: Ok(json!([notification(
                "textDocument/publishDiagnostics",
                json!({"uri":"file:///stale","diagnostics":[]})
            )])),
        }))
        .unwrap();
        for message in [
            json!({"jsonrpc":"2.0","id":3,"method":"shutdown"}),
            json!({"jsonrpc":"2.0","method":"exit"}),
        ] {
            tx.send(Event::Message(message)).unwrap();
        }
        let mut output = Vec::new();
        assert_eq!(
            session(&rx, &Arc::default(), None, true, &mut output).unwrap(),
            0
        );
        let mut input = std::io::Cursor::new(output);
        assert_eq!(
            read_message(&mut input).unwrap().unwrap()["error"]["code"],
            -32002
        );
        assert_eq!(
            read_message(&mut input).unwrap().unwrap()["result"]["capabilities"]
                ["codeActionProvider"],
            false
        );
        assert_eq!(
            read_message(&mut input).unwrap().unwrap()["result"],
            json!([])
        );
        assert_eq!(read_message(&mut input).unwrap().unwrap()["id"], 3);
        assert!(read_message(&mut input).unwrap().is_none());
    }
    #[test]
    fn queued_cancellation_answers_once_without_waiting_for_analysis() {
        let (tx, rx) = mpsc::channel();
        let dir = tempfile::tempdir().unwrap();
        let uri = text::uri(&dir.path().join("Makefile"));
        for message in [
            json!({"id":1,"method":"initialize"}),
            json!({"method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"version":1,"text":"all:;\n"}}}),
            json!({"id":"action","method":"textDocument/codeAction","params":{"textDocument":{"uri":uri}}}),
            json!({"method":"$/cancelRequest","params":{"id":"action"}}),
            json!({"id":2,"method":"shutdown"}),
            json!({"method":"exit"}),
        ] {
            let mut message = message;
            message["jsonrpc"] = json!("2.0");
            if message["method"] == "initialize" {
                message["params"] = json!({"capabilities":{"workspace":{"workspaceEdit":{"documentChanges":true}},"textDocument":{"codeAction":{"codeActionLiteralSupport":{}}}}});
            }
            tx.send(Event::Message(message)).unwrap();
        }
        let mut output = Vec::new();
        let work: Work = Arc::default();
        assert_eq!(session(&rx, &work, None, true, &mut output).unwrap(), 0);
        let mut input = std::io::Cursor::new(output);
        let mut responses = Vec::new();
        while let Some(m) = read_message(&mut input).unwrap() {
            responses.push(m);
        }
        let responses: Vec<_> = responses.iter().filter(|m| m["id"] == "action").collect();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0]["error"]["code"], -32800);
        assert!(work.0.lock().unwrap().requests.is_empty());
    }
    #[test]
    fn stale_versions_and_invalid_incremental_changes_leave_buffer_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let uri = text::uri(&dir.path().join("Makefile"));
        let mut s = Snapshot {
            documents: BTreeMap::new(),
            config: None,
            no_config: true,
            cancelled: Arc::default(),
            versioned_actions: false,
        };
        update(
            &mut s,
            "textDocument/didOpen",
            &json!({"textDocument":{"uri":uri,"version":2,"text":"😀"}}),
        )
        .unwrap();
        assert!(!update(
            &mut s,
            "textDocument/didChange",
            &json!({"textDocument":{"uri":uri,"version":1},"contentChanges":[{"text":"stale"}]})
        )
        .unwrap());
        assert!(update(&mut s,"textDocument/didChange",&json!({"textDocument":{"uri":uri,"version":3},"contentChanges":[{"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":2}},"text":"invalid"}]})).is_err());
        assert_eq!(s.documents[&uri].text.as_ref(), "😀");
        assert_eq!(s.documents[&uri].version, 2);
    }
}
