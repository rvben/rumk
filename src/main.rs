use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use colored::Colorize;
use ignore::WalkBuilder;
use rumk::config::Config;
use rumk::diagnostic::{Applicability, Diagnostic, Severity};
use rumk::lint::{self, LintContext};
use rumk::project::Project;
use rumk::{fix, inline_config, rules, source};
use serde::Serialize;
use similar::TextDiff;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SUCCESS: u8 = 0;
const VIOLATIONS_FOUND: u8 = 1;
const TOOL_ERROR: u8 = 2;

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

        /// Only list rules with automatic fixes
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

    /// Also apply the fixes that can change what Make does
    #[arg(long, conflicts_with = "no_unsafe_fixes")]
    unsafe_fixes: bool,

    /// Withhold the fixes that can change what Make does
    #[arg(long)]
    no_unsafe_fixes: bool,

    /// Control which severity causes exit code 1
    #[arg(long, default_value_t, value_enum)]
    fail_on: FailOn,

    #[command(flatten)]
    shared: SharedArgs,
}

impl CheckArgs {
    /// What the command line says about unsafe fixes, or nothing when it says
    /// nothing and the configuration decides.
    fn unsafe_fixes(&self) -> Option<bool> {
        if self.unsafe_fixes {
            Some(true)
        } else if self.no_unsafe_fixes {
            Some(false)
        } else {
            None
        }
    }
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
    /// Whether the file starts with a byte order mark, which stands before
    /// every byte offset this report puts in machine-readable output.
    byte_order_mark: bool,
    fixed_count: usize,
    changed: bool,
    diff: Option<String>,
    state: ReadState,
}

/// How much of a path Rumk managed to read.
#[derive(Clone, Copy, PartialEq)]
enum ReadState {
    /// The file was read and linted, even if its invalid bytes were replaced.
    Linted,
    /// The file could not be read, so nothing in it was checked.
    UnreadableFile,
    /// A directory could not be read, so any Makefile in it was missed.
    UnreadableDirectory,
}

impl ReadState {
    /// Whether Rumk checked nothing behind this path.
    fn unread(self) -> bool {
        !matches!(self, Self::Linted)
    }
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
    /// Whether applying this fix can change what Make does, which decides
    /// whether a run applies it without being asked.
    applicability: &'static str,
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
            apply_shared_args(&mut config, &args.shared, args.unsafe_fixes())?;
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
            // Formatting is not a decision about what Make does, so `fmt` never
            // applies a fix that can change it, whatever the configuration says.
            // `check --fix --unsafe-fixes` is where those are agreed to.
            apply_shared_args(&mut config, &args.shared, Some(false))?;
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

fn apply_shared_args(
    config: &mut Config,
    args: &SharedArgs,
    unsafe_fixes: Option<bool>,
) -> Result<()> {
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
        unsafe_fixes,
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

    let Discovery {
        files,
        unreadable_paths,
    } = discover_files(&paths, config)?;
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
            // A root that cannot be read is reported when the file itself is
            // processed; it just contributes no included files here. A root
            // that is only invalid UTF-8 is loaded from the same lossy decode
            // that lints it, so the files it includes stay contextual.
            let Ok(source) = read_makefile(root) else {
                continue;
            };
            let Ok(project) =
                Project::load_with_root_content(root, source.text, &config.project_options(root))
            else {
                continue;
            };
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
        .filter_map(|path| {
            let project_root = project_roots.contains(path);
            let contextual = project_root || included_files.contains(&path_identity(path));
            process_file(
                path,
                config,
                operation,
                project_root,
                contextual,
                &covered_files,
                args.silent,
            )
            .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    reports.extend(unreadable_paths.into_iter().filter_map(|(path, error)| {
        let message = error.to_string();
        let failure = rules::ReadFailure::Unreadable {
            kind: path_kind(&path),
            error,
        };
        unreadable_report(&path, config, &failure, &message, args.silent)
    }));
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
    // A path that could not be read was never checked, so a clean exit would
    // be a false result: an MK007 error for it fails every command and ignores
    // --fail-on. Lowering the rule's severity is the way to opt out. A file
    // that is only invalid UTF-8 was linted, so it follows --fail-on.
    let unread = reports.iter().any(|report| {
        report.state.unread()
            && report.diagnostics.iter().any(|diagnostic| {
                diagnostic.rule_id == "MK007" && diagnostic.severity == Severity::Error
            })
    });
    Ok(if violations || unread {
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

/// The Makefiles a run covers, plus the paths it was not allowed to look
/// inside.
struct Discovery {
    files: Vec<PathBuf>,
    unreadable_paths: Vec<(PathBuf, std::io::Error)>,
}

fn discover_files(paths: &[PathBuf], config: &Config) -> Result<Discovery> {
    let mut files = BTreeSet::new();
    let mut unreadable_paths = Vec::new();
    let current_dir = std::env::current_dir().context("Failed to determine current directory")?;

    for path in paths {
        let metadata = match std::fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => bail!(
                "Path '{}' is neither a file nor a directory",
                path.display()
            ),
            // A path Rumk may not even inspect, such as one below a directory
            // it cannot search, was asked for by name: it is handed to the
            // reader, which reports the failure as MK007, and the other paths
            // are still checked. Its name says nothing about its type, so the
            // Makefile-name filter does not apply.
            Err(_) => {
                let relative = path.strip_prefix(&current_dir).unwrap_or(path);
                if !config.is_path_excluded(relative) {
                    files.insert(path.clone());
                }
                continue;
            }
        };
        if metadata.is_file() {
            let relative = path.strip_prefix(&current_dir).unwrap_or(path);
            if is_makefile(path) && !config.is_path_excluded(relative) {
                files.insert(path.clone());
            }
            continue;
        }
        if !metadata.is_dir() {
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
            let entry = match entry {
                Ok(entry) => entry,
                // A path Rumk may not read can hide Makefiles, so the run
                // reports it and walks on instead of aborting or reporting a
                // success it cannot vouch for.
                Err(error) => match unreadable_path(error) {
                    Ok(unreadable) => {
                        // A path the configuration excludes hides nothing the
                        // run would have checked, so not reading it costs the
                        // report nothing.
                        let relative = unreadable
                            .0
                            .strip_prefix(path)
                            .unwrap_or(&unreadable.0)
                            .to_path_buf();
                        if !config.is_path_excluded(&relative)
                            && !config.excludes_everything_below(&relative)
                        {
                            unreadable_paths.push(unreadable);
                        }
                        continue;
                    }
                    Err(error) => {
                        return Err(anyhow::Error::new(error)).with_context(|| {
                            format!("Failed to walk directory: {}", path.display())
                        });
                    }
                },
            };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) || !is_makefile(entry.path()) {
                continue;
            }
            let relative = entry.path().strip_prefix(path).unwrap_or(entry.path());
            if !config.is_path_ignored(relative) {
                files.insert(entry.into_path());
            }
        }
    }

    unreadable_paths.sort_by(|left, right| left.0.cmp(&right.0));
    unreadable_paths.dedup_by(|left, right| left.0 == right.0);
    Ok(Discovery {
        files: files.into_iter().collect(),
        unreadable_paths,
    })
}

/// The path and I/O failure of a walk error about one path, or the error itself
/// when it is about the walk as a whole, such as an unreadable ignore file.
fn unreadable_path(error: ignore::Error) -> Result<(PathBuf, std::io::Error), ignore::Error> {
    match error_path(&error).filter(|_| error.io_error().is_some()) {
        Some(path) => match error.into_io_error() {
            Some(io) => Ok((path, os_failure(io))),
            None => unreachable!("an error with an I/O error carries an I/O error"),
        },
        None => Err(error),
    }
}

/// The operating system failure behind a walk error. The walker wraps it in a
/// message that repeats the path Rumk already prints beside the diagnostic, so
/// the original failure is unwrapped whenever it is still reachable.
fn os_failure(io: std::io::Error) -> std::io::Error {
    let code = io.raw_os_error().or_else(|| {
        io.get_ref()
            .and_then(std::error::Error::source)
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .and_then(std::io::Error::raw_os_error)
    });
    code.map_or(io, std::io::Error::from_raw_os_error)
}

/// The path a walk error is about, when it names one.
fn error_path(error: &ignore::Error) -> Option<PathBuf> {
    match error {
        ignore::Error::WithPath { path, .. } => Some(path.clone()),
        ignore::Error::WithLineNumber { err, .. } | ignore::Error::WithDepth { err, .. } => {
            error_path(err)
        }
        _ => None,
    }
}

fn process_file(
    path: &Path,
    config: &Config,
    operation: Operation,
    project_root: bool,
    contextual: bool,
    covered_files: &BTreeSet<PathBuf>,
    silent: bool,
) -> Result<Option<FileReport>> {
    // A symlink names the file to read and to rewrite, so the link is followed
    // once: the read and the write address the same file even if the link is
    // pointed elsewhere in between, and a rewrite replaces the file rather than
    // the link that names it.
    let resolved = path_identity(path);
    let MakefileSource {
        text: original,
        byte_order_mark,
        failure,
    } = match read_makefile(&resolved) {
        Ok(source) => source,
        Err(error) => {
            let message = error.to_string();
            let failure = rules::ReadFailure::Unreadable {
                kind: path_kind(path),
                error,
            };
            return Ok(unreadable_report(path, config, &failure, &message, silent));
        }
    };
    let context = LintContext {
        config,
        path,
        project_root,
        contextual,
        covered_files,
    };
    let mut initial_diagnostics = lint::lint(&original, &context)
        .with_context(|| format!("Failed to parse Makefile: {}", path.display()))?;
    if let Some(failure) = &failure {
        // A lossy decode is never written back, so nothing in it is fixable.
        for diagnostic in &mut initial_diagnostics {
            diagnostic.fixable = false;
            diagnostic.fix = None;
        }
        initial_diagnostics.extend(
            inline_config::apply_inline_suppressions(
                &original,
                read_diagnostics(config, path, failure),
            )
            .map_err(anyhow::Error::msg)?,
        );
        lint::sort_diagnostics(&mut initial_diagnostics);
    }
    let mut diagnostics = initial_diagnostics.clone();
    let mut content = original.clone();
    let mut fixed_diagnostics = Vec::new();
    let mut fixed_count = 0;
    let mut diff = None;

    if operation.applies_fixes() && failure.is_none() {
        let fixed = lint::fix(&content, diagnostics, &context)?;
        content = fixed.content;
        diagnostics = fixed.diagnostics;
        fixed_diagnostics = fixed.applied;

        if content != original {
            fixed_count = fixed_diagnostics.len();
            // The diff and the write both describe the file on disk, which
            // keeps the byte order mark Make reads past.
            diff = Some(render_diff(
                path,
                &with_byte_order_mark(&original, byte_order_mark),
                &with_byte_order_mark(&content, byte_order_mark),
            ));
            if operation.writes() {
                atomic_write(&resolved, &with_byte_order_mark(&content, byte_order_mark))?;
            }
        }
    }

    Ok(Some(FileReport {
        path: display_path(path),
        diagnostics,
        initial_diagnostics,
        fixed_diagnostics,
        changed: content != original,
        content,
        byte_order_mark,
        fixed_count,
        diff,
        state: ReadState::Linted,
    }))
}

/// The report for a path Rumk could not read, or `None` when MK007 is disabled
/// for it, in which case the path is skipped with a warning instead.
fn unreadable_report(
    path: &Path,
    config: &Config,
    failure: &rules::ReadFailure,
    message: &str,
    silent: bool,
) -> Option<FileReport> {
    let state = match failure {
        rules::ReadFailure::Unreadable {
            kind: rules::PathKind::Directory,
            ..
        } => ReadState::UnreadableDirectory,
        _ => ReadState::UnreadableFile,
    };
    let diagnostics = read_diagnostics(config, path, failure);
    if diagnostics.is_empty() {
        // A rule the configuration silences for this path was silenced on
        // purpose, so only a rule disabled everywhere is worth saying.
        if !silent && !read_failure_is_ignored(config, path, failure) {
            eprintln!(
                "warning: {} could not be read and MK007 is disabled: {message}",
                display_path(path)
            );
        }
        return None;
    }
    Some(FileReport {
        path: display_path(path),
        initial_diagnostics: diagnostics.clone(),
        diagnostics,
        fixed_diagnostics: Vec::new(),
        content: String::new(),
        byte_order_mark: false,
        fixed_count: 0,
        changed: false,
        diff: None,
        state,
    })
}

/// A Makefile Rumk managed to read.
struct MakefileSource {
    /// The text Make reads, which every rule and every fix works on.
    text: String,
    /// Whether a byte order mark stood before that text, which a rewrite
    /// writes back so the file keeps the bytes it came with.
    byte_order_mark: bool,
    /// Where the first byte that is not UTF-8 was, when the text had to be
    /// decoded lossily.
    failure: Option<rules::ReadFailure>,
}

/// Reads a Makefile, decoding invalid UTF-8 lossily so that it can still be
/// linted; the failure says where the first invalid byte was.
fn read_makefile(path: &Path) -> std::io::Result<MakefileSource> {
    let read = std::fs::read(path)?;
    let (bytes, byte_order_mark) = source::split_byte_order_mark(&read);
    Ok(match std::str::from_utf8(bytes) {
        Ok(content) => MakefileSource {
            text: content.to_string(),
            byte_order_mark,
            failure: None,
        },
        Err(error) => {
            let (line, column) = text_position(bytes, error.valid_up_to());
            MakefileSource {
                text: String::from_utf8_lossy(bytes).into_owned(),
                byte_order_mark,
                failure: Some(rules::ReadFailure::InvalidUtf8 { line, column }),
            }
        }
    })
}

/// Line and character column, both 1-based, of the byte at `offset`, which
/// follows only valid UTF-8.
fn text_position(bytes: &[u8], offset: usize) -> (usize, usize) {
    let line_start = bytes[..offset]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);
    let line = bytes[..line_start]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let column = std::str::from_utf8(&bytes[line_start..offset])
        .expect("bytes before the first invalid sequence are valid UTF-8")
        .chars()
        .count()
        + 1;
    (line, column)
}

fn read_diagnostics(config: &Config, path: &Path, failure: &rules::ReadFailure) -> Vec<Diagnostic> {
    config
        .rules
        .iter()
        .flat_map(|rule| rule.check_read(failure))
        .filter(|diagnostic| !config.is_rule_ignored_for_path(path, &diagnostic.rule_id))
        .collect()
}

/// Whether an enabled rule reports `failure` but is ignored for `path`.
fn read_failure_is_ignored(config: &Config, path: &Path, failure: &rules::ReadFailure) -> bool {
    config
        .rules
        .iter()
        .flat_map(|rule| rule.check_read(failure))
        .any(|diagnostic| config.is_rule_ignored_for_path(path, &diagnostic.rule_id))
}

fn render_diff(path: &Path, original: &str, fixed: &str) -> String {
    let label = display_path(path);
    TextDiff::from_lines(original, fixed)
        .unified_diff()
        .header(&label, &label)
        .to_string()
}

/// The text as it stands on disk, which is the text Make reads behind the byte
/// order mark the file may start with.
fn with_byte_order_mark(text: &str, present: bool) -> std::borrow::Cow<'_, str> {
    if present {
        std::borrow::Cow::Owned(format!("{}{text}", source::BYTE_ORDER_MARK))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Replaces the file at `path`, which the caller has already resolved, so the
/// temporary file shares its filesystem and a symlink pointing at it survives.
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
    let current_dir = std::env::current_dir()
        .ok()
        .and_then(|path| dunce::canonicalize(path).ok());
    let canonical_path = dunce::canonicalize(path).ok();
    let comparable_path = canonical_path.as_deref().unwrap_or(path);
    let relative = current_dir
        .as_deref()
        .and_then(|current_dir| comparable_path.strip_prefix(current_dir).ok())
        .unwrap_or(comparable_path);
    relative
        .strip_prefix(".")
        .unwrap_or(relative)
        .display()
        .to_string()
}

fn path_identity(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// What Rumk can still tell about a path whose read failed. A path it is not
/// even allowed to inspect has no known kind, so the diagnostic must not claim
/// one.
fn path_kind(path: &Path) -> rules::PathKind {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => rules::PathKind::Directory,
        Ok(metadata) if metadata.is_file() => rules::PathKind::File,
        _ => rules::PathKind::Unknown,
    }
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
            // A path that could not be read has no diff to show, and the run
            // fails because of it, so its diagnostic is printed instead of
            // leaving the failure unexplained.
            if report.state.unread() {
                output_text(report, operation);
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
                let file = diagnostic_path(report, diagnostic);
                // A byte range is only meaningful in the content it was
                // measured against, so a diagnostic another file carries is
                // reported without the edit that would fix it.
                let json_fix = diagnostic
                    .fix
                    .as_ref()
                    .filter(|_| file == report.path)
                    .and_then(|fix| Some((fix.applicability, fix.edits.first()?)))
                    .and_then(|(applicability, edit)| {
                        // Offsets name bytes in the file, which begins with the
                        // byte order mark Make reads past.
                        let mark = if report.byte_order_mark {
                            source::BYTE_ORDER_MARK.len()
                        } else {
                            0
                        };
                        fix::edit_byte_range(&report.content, edit).map(|(start, end)| JsonFix {
                            applicability: applicability.as_str(),
                            range: JsonRange {
                                start: start + mark,
                                end: end + mark,
                            },
                            replacement: &edit.replacement,
                        })
                    });
                JsonDiagnostic {
                    file,
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

    // A fix this run withheld because it can change what Make does. It is
    // still reported, so the summary can say it is there to be asked for.
    let hidden = reports
        .iter()
        .flat_map(|report| &report.diagnostics)
        .filter(|diagnostic| !diagnostic.fixable && diagnostic.fix.is_some())
        .count();

    if operation.shows_diff() {
        // Diff output is a patch other tools read, so the note goes to stderr
        // rather than into the patch. Without it a run whose only fixes are
        // withheld prints nothing at all and still fails.
        if hidden > 0 {
            eprintln!("{}", hidden_fix_hint(hidden));
        }
        return;
    }
    let issue_count: usize = reports.iter().map(|report| report.diagnostics.len()).sum();
    // A directory Rumk could not read is reported, but it is not a file it
    // checked.
    let checked = reports
        .iter()
        .filter(|report| report.state != ReadState::UnreadableDirectory)
        .count();
    if issue_count == 0 {
        println!(
            "{} No issues found in {} {}",
            "✓".green(),
            checked,
            pluralize(checked, "file", "files")
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
            "Found {issue_count} {} in {issue_files} {} ({checked} {} checked)",
            pluralize(issue_count, "issue", "issues"),
            pluralize(issue_files, "file", "files"),
            pluralize(checked, "file", "files")
        );
        let fixable_diagnostics = reports
            .iter()
            .flat_map(|report| &report.diagnostics)
            .filter(|diagnostic| diagnostic.fixable);
        let mut fixable = 0;
        // A fix counted here because this run asked for unsafe fixes is only
        // applied by a run that asks again, so the command says so.
        let mut needs_unsafe = false;
        for diagnostic in fixable_diagnostics {
            fixable += 1;
            needs_unsafe |= diagnostic
                .fix
                .as_ref()
                .is_some_and(|fix| fix.applicability == Applicability::Unsafe);
        }
        if fixable > 0 {
            let command = if needs_unsafe {
                "rumk check --fix --unsafe-fixes"
            } else {
                "rumk check --fix"
            };
            println!(
                "Run `{}` to fix {fixable} {}",
                command.green(),
                pluralize(fixable, "issue", "issues")
            );
        }
        if hidden > 0 {
            println!("{}", hidden_fix_hint(hidden));
        }
    }
}

/// Says that fixes exist which this run withheld, and how to ask for them.
fn hidden_fix_hint(hidden: usize) -> String {
    format!(
        "{hidden} {} can change what Make does; apply with `{}`",
        pluralize(hidden, "fix", "fixes"),
        "rumk check --fix --unsafe-fixes".green()
    )
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
        println!("{} - {}", rule.id(), rule.name());
        println!("Category: {}", rule.category().as_str());
        println!(
            "Default: {}",
            if enabled_by_default {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!(
            "Fixable: {}",
            if rule.fixable() {
                format!("yes ({} fix)", rule.fix_applicability().as_str())
            } else {
                "no".to_string()
            }
        );
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
