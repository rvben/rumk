use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use rumk::config::Config;
use rumk::diagnostic::{Diagnostic, Severity};
use rumk::{fix, parser, rules};
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Parser)]
#[command(name = "rumk")]
#[command(about = "A fast linter for Makefiles", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check {
        #[arg(default_value = "Makefile")]
        path: PathBuf,

        #[arg(short, long)]
        config: Option<PathBuf>,

        #[arg(long, default_value = "text")]
        format: OutputFormat,

        #[arg(long, help = "Fix any fixable issues")]
        fix: bool,
    },
    Explain {
        rule: String,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Github,
}

#[derive(Serialize)]
struct JsonReport<'a> {
    files: &'a [FileReport],
}

#[derive(Serialize)]
struct FileReport {
    path: String,
    diagnostics: Vec<Diagnostic>,

    #[serde(skip)]
    fixed_count: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check {
            path,
            config,
            format,
            fix,
        } => {
            let config = load_config(config)?;
            if check_path(&path, &config, format, fix)? {
                std::process::exit(1);
            }
        }
        Commands::Explain { rule } => explain_rule(&rule)?,
    }

    Ok(())
}

fn load_config(path: Option<PathBuf>) -> Result<Config> {
    match path {
        Some(path) => Config::from_file(&path),
        None => Config::find_and_load(),
    }
}

fn check_path(path: &Path, config: &Config, format: OutputFormat, auto_fix: bool) -> Result<bool> {
    if path.is_file() {
        check_file(path, config, format, auto_fix)
    } else if path.is_dir() {
        check_directory(path, config, format, auto_fix)
    } else {
        anyhow::bail!(
            "Path '{}' is neither a file nor a directory",
            path.display()
        )
    }
}

fn check_directory(
    dir: &Path,
    config: &Config,
    format: OutputFormat,
    auto_fix: bool,
) -> Result<bool> {
    let mut reports = Vec::new();

    for entry in WalkDir::new(dir) {
        let entry =
            entry.with_context(|| format!("Failed to walk directory: {}", dir.display()))?;
        if !entry.file_type().is_file() || !is_makefile(entry.path()) {
            continue;
        }

        let relative_path = entry.path().strip_prefix(dir).unwrap_or(entry.path());
        if config.is_path_ignored(relative_path) {
            continue;
        }

        reports.push(process_file(entry.path(), config, auto_fix)?);
    }

    reports.sort_by(|left, right| left.path.cmp(&right.path));
    output_reports(&reports, format)?;

    if matches!(format, OutputFormat::Text) {
        output_directory_summary(&reports, auto_fix);
    }

    Ok(has_errors(&reports))
}

fn check_file(path: &Path, config: &Config, format: OutputFormat, auto_fix: bool) -> Result<bool> {
    let ignore_path = std::env::current_dir()
        .ok()
        .and_then(|current_dir| path.strip_prefix(current_dir).ok())
        .unwrap_or(path);
    let reports = if config.is_path_ignored(ignore_path) {
        Vec::new()
    } else {
        vec![process_file(path, config, auto_fix)?]
    };

    output_reports(&reports, format)?;

    if matches!(format, OutputFormat::Text) {
        output_file_summary(&reports, auto_fix);
    }

    Ok(has_errors(&reports))
}

fn process_file(path: &Path, config: &Config, auto_fix: bool) -> Result<FileReport> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read Makefile: {}", path.display()))?;
    let mut diagnostics = lint(&content, config)
        .with_context(|| format!("Failed to parse Makefile: {}", path.display()))?;
    let mut fixed_count = 0;

    if auto_fix {
        let fixed_content = fix::apply_fixes(&content, &diagnostics);
        if fixed_content != content {
            fixed_count = diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.fixable)
                .count();
            std::fs::write(path, &fixed_content)
                .with_context(|| format!("Failed to write fixed Makefile: {}", path.display()))?;
            diagnostics = lint(&fixed_content, config)
                .with_context(|| format!("Failed to parse fixed Makefile: {}", path.display()))?;
        }
    }

    Ok(FileReport {
        path: path.display().to_string(),
        diagnostics,
        fixed_count,
    })
}

fn lint(content: &str, config: &Config) -> Result<Vec<Diagnostic>> {
    let makefile = parser::parse(content)?;
    let mut diagnostics = config
        .rules
        .iter()
        .flat_map(|rule| rule.check(&makefile, content))
        .collect::<Vec<_>>();
    diagnostics.sort_by_key(|diagnostic| (diagnostic.line, diagnostic.column));
    Ok(diagnostics)
}

fn is_makefile(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            matches!(name, "Makefile" | "makefile" | "GNUmakefile")
                || name.ends_with(".mk")
                || name.ends_with(".make")
        })
}

fn output_reports(reports: &[FileReport], format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            for report in reports {
                output_text(report);
            }
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&JsonReport { files: reports })?
            );
        }
        OutputFormat::Github => {
            for report in reports {
                output_github(report);
            }
        }
    }
    Ok(())
}

fn output_text(report: &FileReport) {
    for diagnostic in &report.diagnostics {
        let rule_color = match diagnostic.severity {
            Severity::Error => "red",
            Severity::Warning => "yellow",
            Severity::Info => "cyan",
        };
        let fix_indicator = if diagnostic.fixable { " [*]" } else { "" };

        println!(
            "{}:{}:{}: {} {}{}",
            report.path.cyan(),
            diagnostic.line,
            diagnostic.column,
            format!("[{}]", diagnostic.rule_id).color(rule_color),
            diagnostic.message,
            fix_indicator.yellow()
        );
    }
}

fn output_github(report: &FileReport) {
    for diagnostic in &report.diagnostics {
        let level = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "notice",
        };

        println!(
            "::{} file={},line={},col={}::{}",
            level,
            escape_github_property(&report.path),
            diagnostic.line,
            diagnostic.column,
            escape_github_message(&diagnostic.message)
        );
    }
}

fn escape_github_message(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn escape_github_property(value: &str) -> String {
    escape_github_message(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn output_file_summary(reports: &[FileReport], auto_fix: bool) {
    let Some(report) = reports.first() else {
        println!("{} File ignored by configuration", "✓".green());
        return;
    };

    if report.fixed_count > 0 {
        println!(
            "Fixed {} {}",
            report.fixed_count,
            pluralize(report.fixed_count, "issue", "issues")
        );
    }

    if report.diagnostics.is_empty() {
        println!("{} No issues found in {}", "✓".green(), report.path);
        return;
    }

    println!();
    println!(
        "Found {} {} in 1 file (1 file checked)",
        report.diagnostics.len().to_string().red(),
        pluralize(report.diagnostics.len(), "issue", "issues")
    );
    let fixable_count = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.fixable)
        .count();
    if fixable_count > 0 && !auto_fix {
        println!("Run with {} to automatically fix issues", "--fix".green());
    }
}

fn output_directory_summary(reports: &[FileReport], auto_fix: bool) {
    let fixed_count: usize = reports.iter().map(|report| report.fixed_count).sum();
    if fixed_count > 0 {
        println!(
            "Fixed {} {}",
            fixed_count,
            pluralize(fixed_count, "issue", "issues")
        );
    }

    println!();
    if reports.is_empty() {
        println!("{} No Makefiles found", "✓".green());
        return;
    }

    let total_issues: usize = reports.iter().map(|report| report.diagnostics.len()).sum();
    if total_issues == 0 {
        println!(
            "{} All {} {} checked successfully",
            "✓".green(),
            reports.len(),
            pluralize(reports.len(), "file", "files")
        );
        return;
    }

    let files_with_issues = reports
        .iter()
        .filter(|report| !report.diagnostics.is_empty())
        .count();
    println!(
        "Found {} {} in {} {} ({} {} checked)",
        total_issues.to_string().red(),
        pluralize(total_issues, "issue", "issues"),
        files_with_issues.to_string().red(),
        pluralize(files_with_issues, "file", "files"),
        reports.len(),
        pluralize(reports.len(), "file", "files")
    );

    let fixable_count = reports
        .iter()
        .flat_map(|report| &report.diagnostics)
        .filter(|diagnostic| diagnostic.fixable)
        .count();
    if fixable_count > 0 && !auto_fix {
        println!("Run with {} to automatically fix issues", "--fix".green());
    }
}

fn has_errors(reports: &[FileReport]) -> bool {
    reports.iter().any(|report| {
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    })
}

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn explain_rule(rule_id: &str) -> Result<()> {
    println!("{}", rules::get_rule_explanation(rule_id)?);
    Ok(())
}
