use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use colored::Colorize;
use ignore::WalkBuilder;
use rumk::config::Config;
use rumk::diagnostic::{Diagnostic, Severity};
use rumk::project::Project;
use rumk::{fix, inline_config, parser, rules};
use serde::Serialize;
use similar::TextDiff;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const VIOLATIONS_FOUND: u8 = 1;
const TOOL_ERROR: u8 = 2;
const MAX_FIX_ITERATIONS: usize = 10;

#[derive(Parser)]
#[command(name = "rumk", author, version, about = "A fast linter for Makefiles")]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to a configuration file
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Ignore configuration files and use built-in defaults
    #[arg(long, global = true, visible_alias = "isolated")]
    no_config: bool,

    /// Control colored output
    #[arg(long, global = true, default_value_t, value_enum)]
    color: Color,
}

#[derive(Subcommand)]
enum Commands {
    /// Lint Makefiles and print violations
    Check(CheckArgs),
    /// Format Makefiles using all enabled safe fixes
    Fmt(FmtArgs),
    /// Create a starter .rumk.toml configuration
    Init {
        /// Output file path
        #[arg(short, long, default_value = ".rumk.toml")]
        output: PathBuf,
    },
    /// Show information about a rule or list all rules
    Rule {
        rule: Option<String>,

        /// Only list rules with safe automatic fixes
        #[arg(short, long)]
        fixable: bool,

        /// Filter the rule list by category
        #[arg(long)]
        category: Option<String>,

        /// List available rule categories
        #[arg(long)]
        list_categories: bool,
    },
    /// Explain a rule with its rationale
    Explain { rule: String },
    /// Show or query the effective configuration
    Config {
        #[command(subcommand)]
        subcommand: Option<ConfigCommand>,

        /// Show built-in defaults
        #[arg(long)]
        defaults: bool,

        /// Show only settings differing from defaults
        #[arg(long)]
        no_defaults: bool,

        /// Output format for the effective configuration
        #[arg(long, default_value_t, value_enum)]
        output: ConfigOutput,
    },
    /// Show detailed version information
    Version,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Query one effective configuration key
    Get { key: String },
    /// Show the loaded configuration file
    File,
}

#[derive(Args)]
struct CheckArgs {
    /// Files or directories to check; defaults to the current directory
    paths: Vec<PathBuf>,

    /// Fix issues automatically where possible
    #[arg(short, long, conflicts_with = "diff")]
    fix: bool,

    /// Show the diff of available fixes without writing files
    #[arg(long)]
    diff: bool,

    /// Control which severity causes exit code 1
    #[arg(long, default_value_t, value_enum)]
    fail_on: FailOn,

    #[command(flatten)]
    shared: SharedArgs,
}

#[derive(Args)]
struct FmtArgs {
    /// Files or directories to format; defaults to the current directory
    paths: Vec<PathBuf>,

    /// Show formatting changes without writing files
    #[arg(long, conflicts_with = "check")]
    diff: bool,

    /// Fail if formatting changes are required, without writing files
    #[arg(long)]
    check: bool,

    #[command(flatten)]
    shared: SharedArgs,
}

#[derive(Args, Default)]
struct SharedArgs {
    /// Disable specific rules (comma-separated)
    #[arg(short, long)]
    disable: Option<String>,

    /// Enable only specific rules (comma-separated)
    #[arg(short, long, visible_alias = "rules")]
    enable: Option<String>,

    /// Add rules to the enabled set (comma-separated)
    #[arg(long)]
    extend_enable: Option<String>,

    /// Add rules to the disabled set (comma-separated)
    #[arg(long)]
    extend_disable: Option<String>,

    /// Only allow these rules to be fixed (comma-separated)
    #[arg(long)]
    fixable: Option<String>,

    /// Prevent these rules from being fixed (comma-separated)
    #[arg(long)]
    unfixable: Option<String>,

    /// Exclude file patterns (comma-separated)
    #[arg(long)]
    exclude: Option<String>,

    /// Include only file patterns (comma-separated)
    #[arg(long)]
    include: Option<String>,

    /// Disable configured excludes
    #[arg(long)]
    no_exclude: bool,

    /// Respect .gitignore files while scanning directories
    #[arg(long, num_args(0..=1), require_equals = true, default_missing_value = "true")]
    respect_gitignore: Option<bool>,

    /// Suppress summary output
    #[arg(short, long)]
    quiet: bool,

    /// Suppress diagnostics and summaries
    #[arg(short, long)]
    silent: bool,

    /// Output format for diagnostics
    #[arg(long, visible_alias = "format", default_value_t, value_enum)]
    output_format: OutputFormat,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum OutputFormat {
    #[default]
    Text,
    Json,
    #[value(name = "github")]
    GitHub,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum ConfigOutput {
    #[default]
    Toml,
    Json,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum FailOn {
    #[default]
    Any,
    Warning,
    Error,
    Never,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Color {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy)]
enum Operation {
    Check,
    CheckFix,
    CheckDiff,
    Format,
    FormatDiff,
    FormatCheck,
}

impl Operation {
    fn applies_fixes(self) -> bool {
        !matches!(self, Self::Check)
    }

    fn writes(self) -> bool {
        matches!(self, Self::CheckFix | Self::Format)
    }

    fn shows_diff(self) -> bool {
        matches!(self, Self::CheckDiff | Self::FormatDiff | Self::FormatCheck)
    }
}

struct FileReport {
    path: String,
    diagnostics: Vec<Diagnostic>,
    initial_diagnostics: Vec<Diagnostic>,
    fixed_diagnostics: Vec<Diagnostic>,
    content: String,
    fixed_count: usize,
    changed: bool,
    diff: Option<String>,
}

#[derive(Serialize)]
struct JsonDiagnostic<'a> {
    file: String,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
    rule: &'a str,
    message: &'a str,
    severity: &'static str,
    fixable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<JsonFix<'a>>,
}

#[derive(Serialize)]
struct JsonFix<'a> {
    range: JsonRange,
    replacement: &'a str,
}

#[derive(Serialize)]
struct JsonRange {
    start: usize,
    end: usize,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("rumk failed: {error:#}");
            ExitCode::from(TOOL_ERROR)
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    configure_color(cli.color);

    match cli.command {
        Commands::Init { output } => init_config(&output),
        Commands::Rule {
            rule,
            fixable,
            category,
            list_categories,
        } => show_rule(
            rule.as_deref(),
            fixable,
            category.as_deref(),
            list_categories,
        ),
        Commands::Explain { rule } => {
            println!("{}", rules::get_rule_explanation(&rule)?);
            Ok(SUCCESS)
        }
        Commands::Version => {
            println!("rumk {}", env!("CARGO_PKG_VERSION"));
            Ok(SUCCESS)
        }
        Commands::Config {
            subcommand,
            defaults,
            no_defaults,
            output,
        } => {
            if defaults && no_defaults {
                bail!("--defaults and --no-defaults cannot be used together");
            }
            let config = load_config(cli.config.as_deref(), cli.no_config)?;
            show_config(&config, subcommand, defaults, no_defaults, output)
        }
        Commands::Check(args) => {
            let mut config = load_config(cli.config.as_deref(), cli.no_config)?;
            apply_shared_args(&mut config, &args.shared)?;
            let operation = if args.fix {
                Operation::CheckFix
            } else if args.diff {
                Operation::CheckDiff
            } else {
                Operation::Check
            };
            run_files(args.paths, &config, &args.shared, operation, args.fail_on)
        }
        Commands::Fmt(args) => {
            let mut config = load_config(cli.config.as_deref(), cli.no_config)?;
            apply_shared_args(&mut config, &args.shared)?;
            let operation = if args.check {
                Operation::FormatCheck
            } else if args.diff {
                Operation::FormatDiff
            } else {
                Operation::Format
            };
            run_files(args.paths, &config, &args.shared, operation, FailOn::Never)
        }
    }
}

fn configure_color(color: Color) {
    match color {
        Color::Auto => colored::control::unset_override(),
        Color::Always => colored::control::set_override(true),
        Color::Never => colored::control::set_override(false),
    }
}

fn load_config(path: Option<&Path>, no_config: bool) -> Result<Config> {
    if no_config {
        if path.is_some() {
            bail!("--config cannot be combined with --no-config");
        }
        Ok(Config::default())
    } else if let Some(path) = path {
        Config::from_file(path)
    } else {
        Config::find_and_load()
    }
}

fn apply_shared_args(config: &mut Config, args: &SharedArgs) -> Result<()> {
    let enable = args.enable.as_deref().map(parse_list);
    let disable = args.disable.as_deref().map(parse_list).unwrap_or_default();
    let extend_enable = args
        .extend_enable
        .as_deref()
        .map(parse_list)
        .unwrap_or_default();
    let extend_disable = args
        .extend_disable
        .as_deref()
        .map(parse_list)
        .unwrap_or_default();
    config.apply_rule_overrides(enable.as_deref(), &disable, &extend_enable, &extend_disable)?;
    config.apply_file_overrides(
        args.include.as_deref().map(parse_list),
        args.exclude.as_deref().map(parse_list),
        args.no_exclude,
        args.respect_gitignore,
    );
    config.apply_fix_overrides(
        args.fixable.as_deref().map(parse_list),
        args.unfixable.as_deref().map(parse_list),
    )
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn run_files(
    mut paths: Vec<PathBuf>,
    config: &Config,
    args: &SharedArgs,
    operation: Operation,
    fail_on: FailOn,
) -> Result<u8> {
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }
    if operation.shows_diff() && !matches!(args.output_format, OutputFormat::Text) {
        bail!("--diff and --check require text output");
    }

    let files = discover_files(&paths, config)?;
    let mut project_roots = paths
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect::<BTreeSet<_>>();
    project_roots.extend(
        files
            .iter()
            .filter(|path| is_primary_makefile(path))
            .cloned(),
    );
    if project_roots.is_empty() && files.len() == 1 {
        project_roots.insert(files[0].clone());
    }
    let mut included_files = BTreeSet::new();
    if config.rules.iter().any(|rule| rule.project_aware()) {
        for root in &project_roots {
            let project = Project::load(root, &config.project_options(root))
                .with_context(|| format!("Failed to load Make project: {}", root.display()))?;
            included_files.extend(
                project
                    .files()
                    .iter()
                    .filter(|file| file.id != project.root())
                    .map(|file| file.path.clone()),
            );
        }
    }
    let covered_files = files.iter().map(|path| path_identity(path)).collect();
    let mut reports = files
        .iter()
        .map(|path| {
            let project_root = project_roots.contains(path);
            let contextual = project_root || included_files.contains(&path_identity(path));
            process_file(
                path,
                config,
                operation,
                project_root,
                contextual,
                &covered_files,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    reports.sort_by(|left, right| left.path.cmp(&right.path));
    deduplicate_diagnostics(&mut reports);

    if !args.silent {
        output_reports(&reports, args.output_format, operation)?;
        if !args.quiet && matches!(args.output_format, OutputFormat::Text) {
            output_summary(&reports, operation);
        }
    }

    let violations = match operation {
        Operation::Format => false,
        Operation::FormatDiff => false,
        Operation::FormatCheck => reports.iter().any(|report| report.changed),
        Operation::CheckDiff => reports
            .iter()
            .flat_map(|report| &report.initial_diagnostics)
            .any(|diagnostic| fail_on.matches(diagnostic.severity)),
        Operation::Check | Operation::CheckFix => reports
            .iter()
            .flat_map(|report| &report.diagnostics)
            .any(|diagnostic| fail_on.matches(diagnostic.severity)),
    };
    Ok(if violations {
        VIOLATIONS_FOUND
    } else {
        SUCCESS
    })
}

impl FailOn {
    fn matches(self, severity: Severity) -> bool {
        match self {
            Self::Any => true,
            Self::Warning => matches!(severity, Severity::Warning | Severity::Error),
            Self::Error => severity == Severity::Error,
            Self::Never => false,
        }
    }
}

fn discover_files(paths: &[PathBuf], config: &Config) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    let current_dir = std::env::current_dir().context("Failed to determine current directory")?;

    for path in paths {
        if path.is_file() {
            let relative = path.strip_prefix(&current_dir).unwrap_or(path);
            if is_makefile(path) && !config.is_path_excluded(relative) {
                files.insert(path.clone());
            }
            continue;
        }
        if !path.is_dir() {
            bail!(
                "Path '{}' is neither a file nor a directory",
                path.display()
            );
        }

        let mut builder = WalkBuilder::new(path);
        builder
            .git_ignore(config.global.respect_gitignore)
            .git_exclude(config.global.respect_gitignore)
            .git_global(config.global.respect_gitignore)
            .ignore(config.global.respect_gitignore)
            .parents(config.global.respect_gitignore);
        for entry in builder.build() {
            let entry =
                entry.with_context(|| format!("Failed to walk directory: {}", path.display()))?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_makefile(entry.path()) {
                continue;
            }
            let relative = entry.path().strip_prefix(path).unwrap_or(entry.path());
            if !config.is_path_ignored(relative) {
                files.insert(entry.into_path());
            }
        }
    }

    Ok(files.into_iter().collect())
}

fn process_file(
    path: &Path,
    config: &Config,
    operation: Operation,
    project_root: bool,
    contextual: bool,
    covered_files: &BTreeSet<PathBuf>,
) -> Result<FileReport> {
    let original = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read Makefile: {}", path.display()))?;
    let initial_diagnostics = lint(
        &original,
        config,
        path,
        project_root,
        contextual,
        covered_files,
    )
    .with_context(|| format!("Failed to parse Makefile: {}", path.display()))?;
    let mut diagnostics = initial_diagnostics.clone();
    let mut content = original.clone();
    let mut fixed_diagnostics = Vec::new();
    let mut fixed_count = 0;
    let mut diff = None;

    if operation.applies_fixes() {
        let mut seen = BTreeSet::from([content.clone()]);
        for iteration in 0..MAX_FIX_ITERATIONS {
            let fixed = fix::apply_fixes(&content, &diagnostics);
            if fixed == content {
                break;
            }
            if !seen.insert(fixed.clone()) {
                bail!("Fix cycle detected while formatting {}", path.display());
            }
            fixed_diagnostics.extend(
                diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.fixable)
                    .cloned(),
            );
            content = fixed;
            diagnostics = lint(
                &content,
                config,
                path,
                project_root,
                contextual,
                covered_files,
            )
            .with_context(|| format!("Failed to parse formatted Makefile: {}", path.display()))?;
            if iteration + 1 == MAX_FIX_ITERATIONS
                && diagnostics.iter().any(|diagnostic| diagnostic.fixable)
            {
                bail!(
                    "Fixes did not stabilize after {MAX_FIX_ITERATIONS} iterations for {}",
                    path.display()
                );
            }
        }

        if content != original {
            fixed_count = fixed_diagnostics.len();
            diff = Some(render_diff(path, &original, &content));
            if operation.writes() {
                atomic_write(path, &content)?;
            }
        }
    }

    Ok(FileReport {
        path: display_path(path),
        diagnostics,
        initial_diagnostics,
        fixed_diagnostics,
        changed: content != original,
        content,
        fixed_count,
        diff,
    })
}

fn lint(
    content: &str,
    config: &Config,
    path: &Path,
    project_root: bool,
    contextual: bool,
    covered_files: &BTreeSet<PathBuf>,
) -> Result<Vec<Diagnostic>> {
    let makefile = parser::parse(content)?;
    let mut diagnostics = config
        .rules
        .iter()
        .filter(|rule| !contextual || !rule.project_aware())
        .flat_map(|rule| rule.check(&makefile, content))
        .filter(|diagnostic| !config.is_rule_ignored_for_path(path, &diagnostic.rule_id))
        .map(|mut diagnostic| {
            if !config.is_rule_fixable(&diagnostic.rule_id) {
                diagnostic.fixable = false;
                diagnostic.fix = None;
            }
            diagnostic
        })
        .collect::<Vec<_>>();
    diagnostics = inline_config::apply_inline_suppressions(content, diagnostics)
        .map_err(anyhow::Error::msg)?;
    if project_root {
        let project = Project::load_with_root_content(
            path,
            content.to_string(),
            &config.project_options(path),
        )?;
        let mut project_diagnostics = config
            .rules
            .iter()
            .flat_map(|rule| rule.check_project(&project))
            .map(|mut diagnostic| {
                if diagnostic.source.is_none() {
                    diagnostic.source = Some(project.file(project.root()).path.clone());
                }
                if !config.is_rule_fixable(&diagnostic.rule_id) {
                    diagnostic.fixable = false;
                    diagnostic.fix = None;
                }
                diagnostic
            })
            .collect::<Vec<_>>();
        for file in project.files().iter().filter(|file| {
            file.id != project.root()
                && !covered_files.contains(&file.path)
                && !config.is_path_ignored(&file.path)
        }) {
            project_diagnostics.extend(
                config
                    .rules
                    .iter()
                    .filter(|rule| !rule.project_aware())
                    .flat_map(|rule| rule.check(&file.makefile, &file.content))
                    .filter(|diagnostic| {
                        !config.is_rule_ignored_for_path(&file.path, &diagnostic.rule_id)
                    })
                    .map(|mut diagnostic| {
                        diagnostic.source = Some(file.path.clone());
                        diagnostic.fixable = false;
                        diagnostic.fix = None;
                        diagnostic
                    }),
            );
        }
        for file in project.files() {
            let source_diagnostics = project_diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.source.as_deref() == Some(file.path.as_path()))
                .filter(|diagnostic| {
                    !config.is_rule_ignored_for_path(&file.path, &diagnostic.rule_id)
                })
                .cloned()
                .collect();
            diagnostics.extend(
                inline_config::apply_inline_suppressions(&file.content, source_diagnostics)
                    .map_err(anyhow::Error::msg)?,
            );
        }
    }
    diagnostics.sort_by_key(|diagnostic| {
        (
            diagnostic.source.clone(),
            diagnostic.line,
            diagnostic.column,
            diagnostic.rule_id.clone(),
        )
    });
    Ok(diagnostics)
}

fn render_diff(path: &Path, original: &str, fixed: &str) -> String {
    let label = display_path(path);
    TextDiff::from_lines(original, fixed)
        .unified_diff()
        .header(&label, &label)
        .to_string()
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let permissions = std::fs::metadata(path)
        .with_context(|| format!("Failed to inspect Makefile: {}", path.display()))?
        .permissions();
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temporary file beside {}", path.display()))?;
    temporary
        .write_all(content.as_bytes())
        .with_context(|| format!("Failed to write temporary file for {}", path.display()))?;
    temporary
        .as_file()
        .set_permissions(permissions)
        .with_context(|| format!("Failed to preserve permissions for {}", path.display()))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("Failed to sync temporary file for {}", path.display()))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("Failed to atomically replace Makefile: {}", path.display()))?;
    Ok(())
}

fn display_path(path: &Path) -> String {
    let current_dir = std::env::current_dir().ok();
    let relative = current_dir
        .as_deref()
        .and_then(|current_dir| path.strip_prefix(current_dir).ok())
        .unwrap_or(path);
    relative
        .strip_prefix(".")
        .unwrap_or(relative)
        .display()
        .to_string()
}

fn path_identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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

fn is_primary_makefile(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "Makefile" | "makefile" | "GNUmakefile"))
}

fn deduplicate_diagnostics(reports: &mut [FileReport]) {
    fn retain_unique(
        report_path: &str,
        diagnostics: &mut Vec<Diagnostic>,
        seen: &mut BTreeSet<(String, usize, usize, String, String)>,
    ) {
        diagnostics.retain(|diagnostic| {
            seen.insert((
                diagnostic
                    .source
                    .as_deref()
                    .map(display_path)
                    .unwrap_or_else(|| report_path.to_string()),
                diagnostic.line,
                diagnostic.column,
                diagnostic.rule_id.clone(),
                diagnostic.message.clone(),
            ))
        });
    }

    let mut current = BTreeSet::new();
    let mut initial = BTreeSet::new();
    for report in reports {
        retain_unique(&report.path, &mut report.diagnostics, &mut current);
        retain_unique(&report.path, &mut report.initial_diagnostics, &mut initial);
    }
}

fn diagnostic_path(report: &FileReport, diagnostic: &Diagnostic) -> String {
    diagnostic
        .source
        .as_deref()
        .map(display_path)
        .unwrap_or_else(|| report.path.clone())
}

fn output_reports(
    reports: &[FileReport],
    format: OutputFormat,
    operation: Operation,
) -> Result<()> {
    if operation.shows_diff() {
        for report in reports {
            if let Some(diff) = &report.diff {
                print!("{diff}");
            }
        }
    }

    match format {
        OutputFormat::Text => {
            if !operation.shows_diff() {
                for report in reports {
                    output_text(report, operation);
                }
            }
        }
        OutputFormat::Json => output_json(reports)?,
        OutputFormat::GitHub => {
            for report in reports {
                output_github(report);
            }
        }
    }
    Ok(())
}

fn output_text(report: &FileReport, operation: Operation) {
    if operation.writes() && report.changed {
        for diagnostic in &report.fixed_diagnostics {
            println!(
                "{}:{}:{}: {} {} {}",
                diagnostic_path(report, diagnostic).cyan(),
                diagnostic.line,
                diagnostic.column,
                format!("[{}]", diagnostic.rule_id).yellow(),
                diagnostic.message,
                "[fixed]".green()
            );
        }
    }

    for diagnostic in &report.diagnostics {
        let rule_color = match diagnostic.severity {
            Severity::Error => "red",
            Severity::Warning => "yellow",
            Severity::Info => "cyan",
        };
        let fix_indicator = if diagnostic.fixable { " [*]" } else { "" };
        println!(
            "{}:{}:{}: {} {}{}",
            diagnostic_path(report, diagnostic).cyan(),
            diagnostic.line,
            diagnostic.column,
            format!("[{}]", diagnostic.rule_id).color(rule_color),
            diagnostic.message,
            fix_indicator.yellow()
        );
    }
}

fn output_json(reports: &[FileReport]) -> Result<()> {
    let diagnostics = reports
        .iter()
        .flat_map(|report| {
            report.diagnostics.iter().map(|diagnostic| {
                let json_fix = diagnostic
                    .fix
                    .as_ref()
                    .filter(|_| diagnostic.source.is_none())
                    .and_then(|fix| fix.edits.first())
                    .and_then(|edit| {
                        fix::edit_byte_range(&report.content, edit).map(|(start, end)| JsonFix {
                            range: JsonRange { start, end },
                            replacement: &edit.replacement,
                        })
                    });
                JsonDiagnostic {
                    file: diagnostic_path(report, diagnostic),
                    line: diagnostic.line,
                    column: diagnostic.column,
                    end_line: diagnostic.end_line.unwrap_or(diagnostic.line),
                    end_column: diagnostic.end_column.unwrap_or(diagnostic.column),
                    rule: &diagnostic.rule_id,
                    message: &diagnostic.message,
                    severity: severity_name(diagnostic.severity),
                    fixable: diagnostic.fixable,
                    fix: json_fix,
                }
            })
        })
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&diagnostics)?);
    Ok(())
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
            escape_github_property(&diagnostic_path(report, diagnostic)),
            diagnostic.line,
            diagnostic.column,
            escape_github_message(&diagnostic.message)
        );
    }
}

fn output_summary(reports: &[FileReport], operation: Operation) {
    let fixed: usize = reports.iter().map(|report| report.fixed_count).sum();
    if fixed > 0 && operation.writes() {
        println!(
            "Fixed {fixed} {} in {} {}",
            pluralize(fixed, "issue", "issues"),
            reports.iter().filter(|report| report.changed).count(),
            pluralize(
                reports.iter().filter(|report| report.changed).count(),
                "file",
                "files"
            )
        );
    }

    if operation.shows_diff() {
        return;
    }
    let issue_count: usize = reports.iter().map(|report| report.diagnostics.len()).sum();
    if issue_count == 0 {
        println!(
            "{} No issues found in {} {}",
            "✓".green(),
            reports.len(),
            pluralize(reports.len(), "file", "files")
        );
    } else {
        let issue_files = reports
            .iter()
            .flat_map(|report| {
                report
                    .diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic_path(report, diagnostic))
            })
            .collect::<BTreeSet<_>>()
            .len();
        println!();
        println!(
            "Found {issue_count} {} in {issue_files} {} ({} {} checked)",
            pluralize(issue_count, "issue", "issues"),
            pluralize(issue_files, "file", "files"),
            reports.len(),
            pluralize(reports.len(), "file", "files")
        );
        if reports
            .iter()
            .flat_map(|report| &report.diagnostics)
            .any(|diagnostic| diagnostic.fixable)
        {
            println!("Run `{}` to automatically fix issues", "rumk fmt".green());
        }
    }
}

fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
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

fn pluralize<'a>(count: usize, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 {
        singular
    } else {
        plural
    }
}

fn init_config(path: &Path) -> Result<u8> {
    if path.exists() {
        bail!("Configuration file already exists: {}", path.display());
    }
    let content = r#"[global]
respect-gitignore = true

[MK101]
line-length = 120
"#;
    std::fs::write(path, content)
        .with_context(|| format!("Failed to create configuration: {}", path.display()))?;
    println!("Created {}", path.display());
    Ok(SUCCESS)
}

fn show_rule(
    rule_id: Option<&str>,
    fixable_only: bool,
    category: Option<&str>,
    list_categories: bool,
) -> Result<u8> {
    if list_categories {
        println!("syntax\nstyle\nbest-practices");
        return Ok(SUCCESS);
    }
    let all_rules = rules::get_all_rules();
    if let Some(rule_id) = rule_id {
        let canonical = rule_id.to_ascii_uppercase();
        let rule = all_rules
            .iter()
            .find(|rule| rule.id() == canonical)
            .with_context(|| format!("Unknown rule: {rule_id}"))?;
        let defaults = Config::default();
        let enabled_by_default = defaults.rules.iter().any(|item| item.id() == rule.id());
        println!("{} — {}", rule.id(), rule.name());
        println!("Category: {}", rule.category().as_str());
        println!(
            "Default: {}",
            if enabled_by_default {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!("Fixable: {}", if rule.fixable() { "yes" } else { "no" });
        println!(
            "Scope: {}",
            if rule.project_aware() {
                "project"
            } else {
                "file"
            }
        );
        let options = defaults
            .rule_options(rule.id())
            .expect("known rule has default settings");
        if !options.is_empty() {
            println!("Configuration defaults:");
            for (key, value) in options {
                println!("  {key} = {value}");
            }
        }
        println!("Documentation: {}", rules::documentation_url(rule.id()));
        println!();
        println!("{}", rule.description());
    } else {
        let category = category.map(normalize_category).transpose()?;
        for rule in all_rules.into_iter().filter(|rule| {
            (!fixable_only || rule.fixable())
                && category.is_none_or(|category| rule.category() == category)
        }) {
            println!("{}  {}", rule.id(), rule.name());
        }
    }
    Ok(SUCCESS)
}

fn normalize_category(category: &str) -> Result<rules::RuleCategory> {
    match category.to_ascii_lowercase().replace('_', "-").as_str() {
        "syntax" => Ok(rules::RuleCategory::Syntax),
        "style" => Ok(rules::RuleCategory::Style),
        "best-practices" | "best-practice" => Ok(rules::RuleCategory::BestPractices),
        _ => bail!("Unknown rule category: {category}"),
    }
}

fn show_config(
    config: &Config,
    subcommand: Option<ConfigCommand>,
    defaults: bool,
    no_defaults: bool,
    output: ConfigOutput,
) -> Result<u8> {
    match subcommand {
        Some(ConfigCommand::Get { key }) => {
            println!(
                "{}",
                config
                    .get(&key)
                    .with_context(|| format!("Unknown configuration key: {key}"))?
            );
        }
        Some(ConfigCommand::File) => match config.source_path() {
            Some(path) => println!("{}", path.display()),
            None => println!("No configuration file found (using built-in defaults)"),
        },
        None => {
            let rendered = config.render(defaults, no_defaults);
            match output {
                ConfigOutput::Toml => print!("{rendered}"),
                ConfigOutput::Json => {
                    let value: toml::Value = toml::from_str(&rendered)?;
                    println!("{}", serde_json::to_string_pretty(&value)?);
                }
            }
        }
    }
    Ok(SUCCESS)
}
