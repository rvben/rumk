use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod parser;
mod rules;
mod config;
mod diagnostic;
mod fix;

use crate::config::Config;
use crate::diagnostic::{Diagnostic, Severity};

#[derive(Parser)]
#[command(name = "rumk")]
#[command(about = "A fast, extensible linter for Makefiles", long_about = None)]
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
        
        #[arg(long)]
        fix: bool,
    },
    Fix {
        #[arg(default_value = "Makefile")]
        path: PathBuf,
        
        #[arg(short, long)]
        config: Option<PathBuf>,
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
        Commands::Check { path, config, format, fix } => {
            let config = load_config(config)?;
            check_file(&path, &config, format, fix)?;
        }
        Commands::Fix { path, config } => {
            let config = load_config(config)?;
            fix_file(&path, &config)?;
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
        None => Config::find_and_load().unwrap_or_else(|_| Config::default()),
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
            println!("Fixed {} issues", diagnostics.iter().filter(|d| d.fixable).count());
        }
    }
    
    output_diagnostics(&diagnostics, format, path);
    
    let has_errors = diagnostics.iter().any(|d| matches!(d.severity, Severity::Error));
    if has_errors {
        std::process::exit(1);
    }
    
    Ok(())
}

fn fix_file(path: &PathBuf, config: &Config) -> Result<()> {
    check_file(path, config, OutputFormat::Text, true)
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
    
    for diag in diagnostics {
        let severity_str = match diag.severity {
            Severity::Error => "error".red().bold(),
            Severity::Warning => "warning".yellow().bold(),
            Severity::Info => "info".blue().bold(),
        };
        
        println!(
            "{}:{}:{}: {}: {} [{}]",
            path.display(),
            diag.line,
            diag.column,
            severity_str,
            diag.message,
            diag.rule_id
        );
    }
    
    if !diagnostics.is_empty() {
        let errors = diagnostics.iter().filter(|d| matches!(d.severity, Severity::Error)).count();
        let warnings = diagnostics.iter().filter(|d| matches!(d.severity, Severity::Warning)).count();
        let infos = diagnostics.iter().filter(|d| matches!(d.severity, Severity::Info)).count();
        
        println!("\nFound {} errors, {} warnings, {} info", errors, warnings, infos);
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