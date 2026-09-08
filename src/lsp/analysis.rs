use super::{text, Document, Snapshot};
use crate::config::Config;
use crate::diagnostic::{Diagnostic, Edit, Severity};
use crate::lint::{self, LintContext};
use crate::project::Project;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

pub struct Report {
    pub text: String,
    pub diagnostics: Vec<Diagnostic>,
}
pub type Reports = BTreeMap<PathBuf, Report>;

pub fn config(snapshot: &Snapshot, path: &Path) -> Result<Config> {
    if snapshot.no_config {
        Ok(Config::default())
    } else if let Some(path) = &snapshot.config {
        Config::from_file(path)
    } else {
        Config::find_from(path.parent().unwrap_or(path))
    }
}

pub fn analyze(snapshot: &Snapshot) -> Result<Reports> {
    let covered: BTreeSet<_> = snapshot
        .documents
        .values()
        .map(|doc| doc.path.clone())
        .collect();
    let overlays: BTreeMap<_, _> = snapshot
        .documents
        .values()
        .map(|doc| (doc.path.clone(), doc.text.clone()))
        .collect();
    let mut reports: Reports = snapshot
        .documents
        .values()
        .map(|doc| {
            (
                doc.path.clone(),
                Report {
                    text: doc.text.to_string(),
                    diagnostics: Vec::new(),
                },
            )
        })
        .collect();
    let mut projects = Vec::new();
    let mut included = BTreeSet::new();
    for doc in snapshot.documents.values() {
        if snapshot.cancelled.load(Ordering::Relaxed) {
            return Ok(BTreeMap::new());
        }
        let config = match config(snapshot, &doc.path) {
            Ok(config) => config,
            Err(error) => {
                reports
                    .get_mut(&doc.path)
                    .unwrap()
                    .diagnostics
                    .push(Diagnostic::new(
                        "configuration",
                        Severity::Error,
                        format!("Invalid Rumk configuration: {error:#}"),
                        1,
                        1,
                    ));
                continue;
            }
        };
        if config.is_path_ignored(&doc.path) {
            continue;
        }
        let mut options = config.project_options(&doc.path);
        options.source_overrides = overlays.clone();
        let project =
            Project::load_with_root_content(&doc.path, source(&doc.text).into(), &options)?;
        included.extend(
            project
                .files()
                .iter()
                .filter(|f| f.id != project.root())
                .map(|f| f.path.clone()),
        );
        projects.push((doc, config, project));
    }
    let no_root = projects
        .iter()
        .all(|(doc, _, _)| included.contains(&doc.path));
    for (index, (doc, config, project)) in projects.iter().enumerate() {
        if snapshot.cancelled.load(Ordering::Relaxed) {
            return Ok(BTreeMap::new());
        }
        let root = !included.contains(&doc.path) || (no_root && index == 0);
        let context = LintContext {
            config,
            path: &doc.path,
            project_root: root,
            // Every open buffer is covered by a project pass, including roots.
            // A second file-local pass would ignore declarations in includes.
            contextual: true,
            layout_only: false,
            covered_files: &covered,
        };
        for diagnostic in lint::lint_project(project, &context)? {
            let path = diagnostic.source.as_ref().unwrap_or(&doc.path).clone();
            let Some(file) = project.files().iter().find(|file| file.path == path) else {
                continue;
            };
            reports
                .entry(path)
                .or_insert_with(|| Report {
                    text: file.content.clone(),
                    diagnostics: Vec::new(),
                })
                .diagnostics
                .push(diagnostic);
        }
    }
    for report in reports.values_mut() {
        lint::sort_diagnostics(&mut report.diagnostics);
        report.diagnostics.dedup_by(|a, b| {
            a.rule_id == b.rule_id
                && a.line == b.line
                && a.column == b.column
                && a.message == b.message
        });
    }
    Ok(reports)
}

fn source(text: &str) -> &str {
    text.strip_prefix(crate::source::BYTE_ORDER_MARK)
        .unwrap_or(text)
}
fn mark(text: &str) -> usize {
    text.len() - source(text).len()
}

pub fn location(text: &str, d: &Diagnostic) -> Value {
    let edit = Edit::new(
        d.line.max(1),
        d.column.max(1),
        d.end_line.unwrap_or(d.line).max(d.line).max(1),
        d.end_column.unwrap_or(d.column).max(1),
        "",
    );
    let (start, end) = crate::fix::edit_byte_range(source(text), &edit).unwrap_or((0, 0));
    text::range(text, start + mark(text), end + mark(text))
}

pub fn diagnostic(text: &str, d: &Diagnostic) -> Value {
    let mut value = json!({"range":location(text,d),"severity": match d.severity { Severity::Error=>1,Severity::Warning=>2,Severity::Info=>3 },
        "code":d.rule_id,"source":"rumk","message":d.message,
        "codeDescription":{"href":crate::rules::documentation_url(&d.rule_id)}});
    if !crate::rules::RULE_IDS.contains(&d.rule_id.as_str()) {
        value.as_object_mut().unwrap().remove("codeDescription");
    }
    value
}

fn replacement(doc: &Document, fixed: &str) -> Value {
    // A versioned whole-buffer replacement keeps interacting fixes atomic.
    json!({"documentChanges":[{"textDocument":{"uri":doc.uri,"version":doc.version},
        "edits":[{"range":text::range(&doc.text,0,doc.text.len()),"newText":fixed}]}]})
}

pub fn selection(text: &str, params: &Value) -> Result<Option<(usize, usize)>> {
    params
        .get("range")
        .map(|range| {
            let start = text::offset(text, &range["start"])?;
            let end = text::offset(text, &range["end"])?;
            anyhow::ensure!(start <= end, "Reversed code action range");
            Ok((start, end))
        })
        .transpose()
}

fn intersects(selected: (usize, usize), region: (usize, usize)) -> bool {
    let (start, end) = selected;
    let (a, b) = region;
    if start == end {
        a <= start && start <= b
    } else if a == b {
        start <= a && a < end
    } else {
        start < b && a < end
    }
}

fn fix_all(snapshot: &Snapshot, doc: &Document, report: &Report) -> Result<String> {
    let mut pending = snapshot.clone();
    let mut current = doc.text.to_string();
    let mut diagnostics = report.diagnostics.clone();
    let mut seen = BTreeSet::from([current.clone()]);
    for _ in 0..10 {
        anyhow::ensure!(
            !snapshot.cancelled.load(Ordering::Relaxed),
            "Request cancelled"
        );
        let applied = crate::fix::apply_fixes(source(&current), &diagnostics);
        let fixed = format!("{}{}", &current[..mark(&current)], applied.content);
        if fixed == current {
            return Ok(current);
        }
        anyhow::ensure!(
            fixed.len() <= super::MAX_BUFFER,
            "Fixed document exceeds buffer limit"
        );
        anyhow::ensure!(seen.insert(fixed.clone()), "Fix cycle detected");
        current = fixed;
        pending.documents.get_mut(&doc.uri).unwrap().text = current.clone().into();
        diagnostics = analyze(&pending)?
            .remove(&doc.path)
            .map_or_else(Vec::new, |r| r.diagnostics);
    }
    anyhow::ensure!(
        !diagnostics.iter().any(|d| d.fixable),
        "Fixes did not stabilize after 10 iterations"
    );
    Ok(current)
}

pub fn request(
    snapshot: &Snapshot,
    method: &str,
    params: &Value,
    reports: &Reports,
) -> Result<Value> {
    let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
    let doc = snapshot
        .documents
        .get(uri)
        .ok_or_else(|| anyhow::anyhow!("Document is not open"))?;
    match method {
        "textDocument/codeAction" => {
            let Some(report) = reports.get(&doc.path) else {
                return Ok(json!([]));
            };
            let only = params["context"]["only"].as_array();
            let permits = |kind: &str| {
                only.is_none_or(|values| {
                    values.iter().any(|v| {
                        v.as_str()
                            .is_some_and(|v| kind == v || kind.starts_with(&format!("{v}.")))
                    })
                })
            };
            let mut actions = Vec::new();
            if permits("quickfix") {
                let selected = selection(&doc.text, params)?;
                for d in &report.diagnostics {
                    if !d.fixable {
                        continue;
                    }
                    let Some(fix) = &d.fix else {
                        continue;
                    };
                    if let Some(selected) = selected {
                        let range = location(&doc.text, d);
                        let diagnostic_range = (
                            text::offset(&doc.text, &range["start"])?,
                            text::offset(&doc.text, &range["end"])?,
                        );
                        let touches_fix = fix
                            .edits
                            .iter()
                            .filter_map(|edit| crate::fix::edit_byte_range(source(&doc.text), edit))
                            .any(|(start, end)| {
                                intersects(
                                    selected,
                                    (start + mark(&doc.text), end + mark(&doc.text)),
                                )
                            });
                        if !intersects(selected, diagnostic_range) && !touches_fix {
                            continue;
                        }
                    }
                    let applied =
                        crate::fix::apply_fixes(source(&doc.text), std::slice::from_ref(d));
                    if applied.fixed.is_empty() {
                        continue;
                    }
                    let fixed = format!("{}{}", &doc.text[..mark(&doc.text)], applied.content);
                    actions.push(
                        json!({"title":fix.description,"kind":"quickfix","isPreferred":fix.applicability == crate::diagnostic::Applicability::Safe,
                        "diagnostics":[diagnostic(&doc.text,d)],"edit":replacement(doc,&fixed)}),
                    );
                }
            }
            if permits("source.fixAll.rumk") {
                let fixed = fix_all(snapshot, doc, report)?;
                if fixed != doc.text.as_ref() {
                    actions.push(json!({"title":"Apply all applicable Rumk fixes","kind":"source.fixAll.rumk","edit":replacement(doc,&fixed)}));
                }
            }
            Ok(json!(actions))
        }
        "textDocument/formatting" => {
            let config = config(snapshot, &doc.path)?;
            if config.is_path_ignored(&doc.path) {
                return Ok(json!([]));
            }
            let covered = BTreeSet::new();
            let context = LintContext {
                config: &config,
                path: &doc.path,
                project_root: false,
                contextual: false,
                layout_only: true,
                covered_files: &covered,
            };
            let diagnostics = lint::lint(source(&doc.text), &context)?;
            let fixed = lint::fix(source(&doc.text), diagnostics, &context)?.content;
            let fixed = format!("{}{}", &doc.text[..mark(&doc.text)], fixed);
            if fixed == doc.text.as_ref() {
                Ok(json!([]))
            } else {
                Ok(json!([{"range":text::range(&doc.text,0,doc.text.len()),"newText":fixed}]))
            }
        }
        "textDocument/documentSymbol" => {
            let parsed = crate::parser::parse(source(&doc.text));
            let mut symbols = Vec::new();
            for (name, kind, line, column) in parsed
                .rules
                .iter()
                .flat_map(|r| {
                    r.targets
                        .iter()
                        .map(move |name| (name, 12, r.line, r.column))
                })
                .chain(
                    parsed
                        .variables
                        .values()
                        .map(|v| (&v.name, 13, v.line, v.column)),
                )
            {
                let d = Diagnostic::new("", Severity::Info, "", line, column);
                let range = location(&doc.text, &d);
                // Flat SymbolInformation is supported even by clients that do not
                // advertise hierarchical document symbols.
                symbols.push(
                    json!({"name":name,"kind":kind,"location":{"uri":doc.uri,"range":range}}),
                );
            }
            Ok(json!(symbols))
        }
        _ => anyhow::bail!("Unsupported request"),
    }
}
