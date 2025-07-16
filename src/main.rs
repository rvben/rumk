use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use walkdir::WalkDir;

mod config;
mod diagnostic;
mod fix;
mod parser;
mod rules;

use crate::config::Config;
use crate::diagnostic::{Diagnostic, Severity};

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
            check_path(&path, &config, format, fix)?;
        }
        Commands::Explain { rule } => {
            explain_rule(&rule)?;
        }
    }

    Ok(())
}

fn load_config(path: Option<PathBuf>) -> Result<Config> {
    match path {
        Some(path) => Config::from_file(&path),
        None => Ok(Config::find_and_load().unwrap_or_else(|_| Config::default())),
    }
}

fn check_path(path: &PathBuf, config: &Config, format: OutputFormat, auto_fix: bool) -> Result<()> {
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
    dir: &PathBuf,
    config: &Config,
    format: OutputFormat,
    auto_fix: bool,
) -> Result<()> {
    use colored::*;

    let mut total_files = 0;
    let mut files_with_issues = 0;
    let mut total_issues = 0;
    let mut has_errors = false;

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // Check if this looks like a Makefile
        if is_makefile(path) {
            total_files += 1;

            match std::fs::read_to_string(path) {
                Ok(content) => match parser::parse(&content) {
                    Ok(makefile) => {
                        let mut diagnostics = Vec::new();

                        for rule in &config.rules {
                            let rule_diagnostics = rule.check(&makefile, &content);
                            diagnostics.extend(rule_diagnostics);
                        }

                        diagnostics.sort_by_key(|d| (d.line, d.column));

                        if auto_fix && !diagnostics.is_empty() {
                            let fixed_content = fix::apply_fixes(&content, &diagnostics);
                            if fixed_content != content {
                                std::fs::write(path, fixed_content)?;
                            }
                        }

                        if !diagnostics.is_empty() {
                            files_with_issues += 1;
                            total_issues += diagnostics.len();
                            has_errors = has_errors
                                || diagnostics
                                    .iter()
                                    .any(|d| matches!(d.severity, diagnostic::Severity::Error));
                        }

                        output_diagnostics(&diagnostics, format, &path.to_path_buf());
                    }
                    Err(e) => {
                        eprintln!(
                            "{}: Failed to parse: {}",
                            path.display().to_string().red(),
                            e
                        );
                        files_with_issues += 1;
                        has_errors = true;
                    }
                },
                Err(e) => {
                    eprintln!(
                        "{}: Failed to read: {}",
                        path.display().to_string().red(),
                        e
                    );
                    files_with_issues += 1;
                    has_errors = true;
                }
            }
        }
    }

    // Print summary for text format
    if matches!(format, OutputFormat::Text) && total_files > 0 {
        println!();
        if total_issues == 0 {
            println!(
                "{} All {} {} checked successfully",
                "✓".green(),
                total_files,
                if total_files == 1 { "file" } else { "files" }
            );
        } else {
            println!(
                "Found {} {} in {} {} ({} {} checked)",
                total_issues.to_string().red(),
                if total_issues == 1 { "issue" } else { "issues" },
                files_with_issues.to_string().red(),
                if files_with_issues == 1 {
                    "file"
                } else {
                    "files"
                },
                total_files,
                if total_files == 1 { "file" } else { "files" }
            );

            if !auto_fix {
                println!("Run with {} to automatically fix issues", "--fix".green());
            }
        }
    }

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

fn is_makefile(path: &std::path::Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        // Common Makefile names
        matches!(name, "Makefile" | "makefile" | "GNUmakefile") ||
        // Common extensions
        name.ends_with(".mk") || name.ends_with(".make")
    } else {
        false
    }
}

fn check_file(path: &PathBuf, config: &Config, format: OutputFormat, auto_fix: bool) -> Result<()> {
    let content = std::fs::read_to_string(path)?;
    let makefile = parser::parse(&content)?;

    let mut diagnostics = Vec::new();

    for rule in &config.rules {
        let rule_diagnostics = rule.check(&makefile, &content);
        diagnostics.extend(rule_diagnostics);
    }

    diagnostics.sort_by_key(|d| (d.line, d.column));

    if auto_fix {
        let fixed_content = fix::apply_fixes(&content, &diagnostics);
        if fixed_content != content {
            std::fs::write(path, fixed_content)?;
            println!(
                "Fixed {} issues",
                diagnostics.iter().filter(|d| d.fixable).count()
            );
        }
    }

    output_diagnostics(&diagnostics, format, path);

    // Print summary for text format
    if matches!(format, OutputFormat::Text) && !diagnostics.is_empty() {
        use colored::*;

        let issue_count = diagnostics.len();
        let fixable_count = diagnostics.iter().filter(|d| d.fixable).count();

        println!();
        println!(
            "Found {} {} in 1 file (1 file checked)",
            issue_count.to_string().red(),
            if issue_count == 1 { "issue" } else { "issues" }
        );

        if fixable_count > 0 && !auto_fix {
            println!("Run with {} to automatically fix issues", "--fix".green());
        }
    }

    let has_errors = diagnostics
        .iter()
        .any(|d| matches!(d.severity, Severity::Error));
    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

fn output_diagnostics(diagnostics: &[Diagnostic], format: OutputFormat, path: &PathBuf) {
    match format {
        OutputFormat::Text => output_text(diagnostics, path),
        OutputFormat::Json => output_json(diagnostics),
        OutputFormat::Github => output_github(diagnostics, path),
    }
}

fn output_text(diagnostics: &[Diagnostic], path: &PathBuf) {
    use colored::*;

    if diagnostics.is_empty() {
        println!("{} No issues found in {}", "✓".green(), path.display());
        return;
    }

    for diag in diagnostics {
        let rule_color = match diag.severity {
            Severity::Error => "red",
            Severity::Warning => "yellow",
            Severity::Info => "cyan",
        };

        // Format: filename:line:column: [RULE_ID] message [*]
        let fix_indicator = if diag.fixable { " [*]" } else { "" };

        println!(
            "{}:{}:{}: {} {}{}",
            path.display().to_string().cyan(),
            diag.line,
            diag.column,
            format!("[{}]", diag.rule_id).color(rule_color),
            diag.message,
            fix_indicator.yellow()
        );
    }
}

fn output_json(diagnostics: &[Diagnostic]) {
    let json = serde_json::to_string_pretty(diagnostics).unwrap();
    println!("{}", json);
}

fn output_github(diagnostics: &[Diagnostic], path: &PathBuf) {
    for diag in diagnostics {
        let level = match diag.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "notice",
        };

        println!(
            "::{} file={},line={},col={}::{}",
            level,
            path.display(),
            diag.line,
            diag.column,
            diag.message
        );
    }
}

fn explain_rule(rule_id: &str) -> Result<()> {
    let explanation = rules::get_rule_explanation(rule_id)?;
    println!("{}", explanation);
    Ok(())
}
