use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rumk::diagnostic::Severity;
use rumk::fix::apply_fixes;
use rumk::parser::{parse, VariableScope};
use rumk::project::{Project, ProjectOptions};
use rumk::rules::project::MissingInclude;
use rumk::rules::style::LineLength;
use rumk::rules::syntax::{InvalidSyntax, TabInRecipe};
use rumk::rules::Rule;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gnu_make")
        .join(name)
}

#[test]
fn advanced_fixture_is_accepted_by_gnu_make() {
    let path = fixture("advanced.mk");
    let output = match Command::new("make")
        .args(["--no-builtin-rules", "--dry-run", "-f"])
        .arg(&path)
        .arg("validate")
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    };

    assert!(
        output.status.success(),
        "GNU Make rejected {}:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn portable_entry_marker_whitespace_is_accepted_by_gnu_make() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    for marker in [".POSIX : # mode", ".POSIX\t:\t", ".POSIX \\\n :"] {
        std::fs::write(&path, format!("{marker}\nall:\n\t@:\n")).unwrap();
        let output = match Command::new("make")
            .current_dir(directory.path())
            .args(["--no-builtin-rules", "--dry-run", "-f"])
            .arg(&path)
            .arg("all")
            .output()
        {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("failed to launch GNU Make: {error}"),
        };
        assert!(
            output.status.success(),
            "{marker}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn wrapped_phony_fix_is_accepted_by_gnu_make() {
    let content = concat!(
        ".PHONY: build test lint clean release\n",
        "build test lint clean release:\n",
        "\t@:\n",
    );
    let diagnostics = LineLength::new(32).check(&parse(content), content);
    let fixed = apply_fixes(content, &diagnostics).content;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Makefile");
    std::fs::write(&path, &fixed).unwrap();

    let output = match Command::new("make")
        .args(["--no-builtin-rules", "--dry-run", "-f"])
        .arg(&path)
        .arg("build")
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    };

    assert!(
        output.status.success(),
        "GNU Make rejected the wrapped .PHONY declaration:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let makefile = parse(&fixed);
    assert!(["build", "test", "lint", "clean", "release"]
        .iter()
        .all(|target| makefile.phonies.iter().any(|phony| phony == target)));
}

#[test]
fn recipe_prefix_fix_is_accepted_by_gnu_make() {
    // GNU Make 3.82 introduced .RECIPEPREFIX.
    if !matches!(installed_make_version(), Some(version) if version >= (3, 82)) {
        return;
    }
    let content = ".RECIPEPREFIX := >\nall:\n    @echo prefixed\n";
    let diagnostics = TabInRecipe.check(&parse(content), content);
    let fixed = apply_fixes(content, &diagnostics).content;
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("broken.mk"), content).unwrap();
    std::fs::write(directory.path().join("fixed.mk"), &fixed).unwrap();
    let run = |name: &str| {
        Command::new("make")
            .current_dir(directory.path())
            .args(["--no-builtin-rules", "-f", name, "all"])
            .output()
            .unwrap_or_else(|error| panic!("failed to launch GNU Make: {error}"))
    };

    let broken = run("broken.mk");
    assert!(
        !broken.status.success(),
        "GNU Make accepted the space-indented recipe under a custom prefix"
    );
    let output = run("fixed.mk");
    assert!(
        output.status.success(),
        "GNU Make rejected the fixed recipe:\n{fixed}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "prefixed");
}

#[test]
fn advanced_fixture_has_the_expected_rumk_structure() {
    let path = fixture("advanced.mk");
    let source = std::fs::read_to_string(path).unwrap();
    let makefile = parse(&source);

    assert_eq!(makefile.includes.len(), 1);
    assert!(makefile.includes[0].optional);
    assert_eq!(makefile.conditionals.len(), 3);
    assert_eq!(makefile.definitions.len(), 1);
    assert_eq!(makefile.definitions[0].name, "banner");

    let validate = makefile
        .rules
        .iter()
        .find(|rule| rule.targets == ["validate"] && rule.target_assignment.is_some())
        .unwrap();
    let assignment = validate.target_assignment.as_ref().unwrap();
    assert_eq!(assignment.name, "LABEL");
    assert_eq!(assignment.value, "$(subst x,x,value:with=delimiters)");
    assert_eq!(
        assignment.scope,
        VariableScope::TargetSpecific(vec!["validate".into()])
    );

    let static_pattern = makefile
        .rules
        .iter()
        .find(|rule| rule.target_pattern.is_some())
        .unwrap();
    assert_eq!(static_pattern.target_pattern.as_deref(), Some("%.o"));
    assert_eq!(static_pattern.prerequisites, ["%.c"]);
}

#[test]
fn project_fixture_is_accepted_by_gnu_make_and_rumk() {
    let directory = fixture("project");
    let output = match Command::new("make")
        .current_dir(&directory)
        .args([
            "--no-builtin-rules",
            "--dry-run",
            "-f",
            "Makefile",
            "validate",
        ])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    };
    assert!(
        output.status.success(),
        "GNU Make rejected project fixture:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = Project::load(&directory.join("Makefile"), &ProjectOptions::default()).unwrap();
    assert_eq!(project.files().len(), 3);
    assert!(project.edges().iter().all(|edge| matches!(
        edge.resolution,
        rumk::project::IncludeResolution::Resolved(_)
    )));
}

#[test]
fn safe_evaluator_matches_gnu_make_on_a_controlled_project() {
    let directory = fixture("evaluator");
    let output = match Command::new("make")
        .current_dir(&directory)
        .args(["--no-builtin-rules", "-f", "Makefile", "verify"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    };
    assert!(
        output.status.success(),
        "GNU Make evaluator fixture failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let project = Project::load(&directory.join("Makefile"), &ProjectOptions::default()).unwrap();
    assert_eq!(project.files().len(), 3);
    assert_eq!(
        project
            .edges()
            .iter()
            .filter_map(|edge| edge.expanded.as_deref())
            .collect::<Vec<_>>(),
        ["mk/one.mk", "mk/two.mk"]
    );
    assert_eq!(
        project.evaluation().expand("$(FILES)").as_known(),
        Some("mk/one.mk mk/two.mk")
    );
    assert_eq!(
        project.evaluation().expand("$(OBJECTS)").as_known(),
        Some("src/one.o src/two.cc README")
    );
    assert_eq!(
        project.evaluation().expand("$(WINDOW)").as_known(),
        Some("two three")
    );
    assert_eq!(
        project.evaluation().expand("$(PATHS)").as_known(),
        Some("src/ ./")
    );
    assert_eq!(
        project.evaluation().expand("$(BASENAMES)").as_known(),
        Some("main.c README ")
    );
    assert_eq!(
        project.evaluation().expand("$(LOGICAL)").as_known(),
        Some("selected")
    );
    let all = project.analysis().target("all").unwrap();
    assert_eq!(
        all.dependencies
            .iter()
            .map(|dependency| dependency.prerequisite.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
}

/// GNU Make expands a variable that has no value yet to nothing, so an include
/// placed above the definition reads a path the author never wrote. MK206 names
/// that path, and this holds the claim to what Make does with the same project.
#[test]
fn includes_read_before_their_variable_is_defined_fail_in_gnu_make_where_mk206_reports_them() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("sub/x.mk"), "all:\n\t@:\n").unwrap();

    std::fs::write(&root, "include $(DIR)/x.mk\nDIR := sub\n").unwrap();
    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].severity, Severity::Error);
    assert!(diagnostics[0].message.contains("cannot find '/x.mk'"));
    let Some(failed) = dry_run(directory.path()) else {
        return;
    };
    assert!(
        !failed.status.success(),
        "GNU Make accepted an include read before its variable is defined"
    );
    // The path Make reports is the one the diagnostic names, which is the point
    // of the report: the file says 'sub/x.mk' and Make reads '/x.mk'.
    let error = String::from_utf8_lossy(&failed.stderr);
    assert!(error.contains("/x.mk"), "{error}");
    assert!(!error.contains("sub/x.mk"), "{error}");

    std::fs::write(&root, "DIR := sub\ninclude $(DIR)/x.mk\n").unwrap();
    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let accepted = dry_run(directory.path()).unwrap();
    assert!(
        accepted.status.success(),
        "GNU Make rejected the same project with the definition first:\n{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
}

#[test]
fn gnu_make_reads_an_escaped_include_path_as_one_file_and_mk206_agrees() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");
    std::fs::write(directory.path().join("foo bar.mk"), "all:\n\t@:\n").unwrap();

    for source in [
        "include foo\\ bar.mk\n",
        "X := foo\\ bar.mk\ninclude $(X)\n",
        "include $(EMPTY)foo\\ bar.mk\n",
    ] {
        std::fs::write(&root, source).unwrap();
        let diagnostics = MissingInclude
            .check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());
        assert!(diagnostics.is_empty(), "{source:?}: {diagnostics:?}");
        let Some(accepted) = dry_run(directory.path()) else {
            return;
        };
        assert!(
            accepted.status.success(),
            "GNU Make rejected {source:?}:\n{}",
            String::from_utf8_lossy(&accepted.stderr)
        );
    }

    // The escape is not what makes an include resolve: an escaped path that is
    // really absent is reported, under the name Make itself reads.
    std::fs::write(&root, "include zzz\\ qqq.mk\nall:\n\t@:\n").unwrap();
    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("'zzz qqq.mk'"));
    let rejected = dry_run(directory.path()).unwrap();
    let error = String::from_utf8_lossy(&rejected.stderr);
    assert!(error.contains("zzz qqq.mk"), "{error}");

    // Make halves the run of backslashes in front of a comment as it
    // recognizes the comment, so it reads one backslash less than the line
    // carries.
    std::fs::write(&root, "include zzz\\\\#comment\nall:\n\t@:\n").unwrap();
    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("'zzz\\'"),
        "{}",
        diagnostics[0].message
    );
    let rejected = dry_run(directory.path()).unwrap();
    let error = String::from_utf8_lossy(&rejected.stderr);
    assert!(error.contains("zzz\\:"), "{error}");
}

#[test]
fn gnu_make_reads_an_include_through_a_variable_that_waited_and_mk206_names_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("Makefile");

    // Each variable is computed where LATER still has no value, so it holds
    // what Make put in it there however the file reads on.
    for (source, read) in [
        (
            "DIR := $(LATER)\ninclude $(DIR)/x.mk\nLATER := sub\nall:\n\t@:\n",
            "/x.mk",
        ),
        (
            "DIR := $(LATER)\nDIR += x\ninclude $(DIR)/y.mk\nLATER := sub\nall:\n\t@:\n",
            "x/y.mk",
        ),
        (
            "DIR :=\nDIR += $(LATER)sub\ninclude $(DIR)/y.mk\nLATER := q\nall:\n\t@:\n",
            "sub/y.mk",
        ),
    ] {
        std::fs::write(&root, source).unwrap();

        let diagnostics = MissingInclude
            .check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());

        assert_eq!(diagnostics.len(), 1, "{source:?}: {diagnostics:?}");
        assert_eq!(diagnostics[0].severity, Severity::Error, "{source:?}");
        assert!(
            diagnostics[0].message.contains(&format!("'{read}'")),
            "{source:?}: {}",
            diagnostics[0].message
        );
        let Some(rejected) = dry_run(directory.path()) else {
            return;
        };
        let error = String::from_utf8_lossy(&rejected.stderr);
        assert!(!rejected.status.success(), "{source:?}: {error}");
        assert!(error.contains(read), "{source:?}: {error}");
    }

    // A definition that comes before the include can still come too late: Make
    // reads a file here, just not the one below was meant to point at.
    std::fs::create_dir(directory.path().join("sub")).unwrap();
    std::fs::write(directory.path().join("config.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("sub/config.mk"), "WHICH := sub\n").unwrap();
    std::fs::write(
        &root,
        "FILES := $(LATER)config.mk\nLATER := sub/\ninclude $(FILES)\nall:\n\t@echo read=$(WHICH)\n",
    )
    .unwrap();

    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0].message.contains("so Make reads 'config.mk'"),
        "{}",
        diagnostics[0].message
    );
    let accepted = dry_run(directory.path()).unwrap();
    let read = String::from_utf8_lossy(&accepted.stdout);
    assert!(accepted.status.success(), "{read}");
    assert!(read.contains("read=root"), "{read}");

    // A name Make read a definition for and then gave back has no value here
    // either, and the definition below is still one it reaches too late.
    std::fs::write(
        &root,
        "DIR := old/\nundefine DIR\ninclude $(DIR)config.mk\nDIR := sub/\nall:\n\t@echo read=$(WHICH)\n",
    )
    .unwrap();

    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]
            .message
            .contains("expands 'DIR' before line 4 defines it, so Make reads 'config.mk'"),
        "{}",
        diagnostics[0].message
    );
    // GNU Make 3.82 introduced `undefine`.
    let modern = matches!(installed_make_version(), Some(version) if version >= (3, 82));
    if modern {
        let accepted = dry_run(directory.path()).unwrap();
        let read = String::from_utf8_lossy(&accepted.stdout);
        assert!(accepted.status.success(), "{read}");
        assert!(read.contains("read=root"), "{read}");
    }

    // Make reads a file included twice from the top both times, so a
    // definition in it is one it reaches again after the include above it.
    std::fs::write(directory.path().join("x.mk"), "WHICH := root\n").unwrap();
    std::fs::write(directory.path().join("sub/x.mk"), "WHICH := sub\n").unwrap();
    std::fs::write(
        directory.path().join("shared.mk"),
        "include $(DIR)x.mk\nDIR := sub/\n",
    )
    .unwrap();
    std::fs::write(
        &root,
        "DIR := sub/\ninclude shared.mk\nundefine DIR\ninclude shared.mk\nall:\n\t@echo read=$(WHICH)\n",
    )
    .unwrap();

    let diagnostics =
        MissingInclude.check_project(&Project::load(&root, &ProjectOptions::default()).unwrap());

    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]
            .message
            .contains("expands 'DIR' before line 2 defines it, so Make reads 'x.mk'"),
        "{}",
        diagnostics[0].message
    );
    if modern {
        let accepted = dry_run(directory.path()).unwrap();
        let read = String::from_utf8_lossy(&accepted.stdout);
        assert!(accepted.status.success(), "{read}");
        // The second reading of shared.mk is the one that lands, and it read
        // the file in this directory.
        assert!(read.contains("read=root"), "{read}");
    }
}

/// What GNU Make makes of `all` in `directory`, and `None` where GNU Make is
/// not installed.
fn dry_run(directory: &Path) -> Option<Output> {
    match Command::new("make")
        .current_dir(directory)
        .args(["--no-builtin-rules", "--dry-run", "-f", "Makefile", "all"])
        .output()
    {
        Ok(output) => Some(output),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    }
}

/// A Makefile under `tests/fixtures/gnu_make/syntax` whose leading
/// `# key: value` comments state what GNU Make and MK006 must agree on.
struct SyntaxFixture {
    path: PathBuf,
    name: String,
    source: String,
    /// The lowest GNU Make version the fixture was verified against. An
    /// older installation skips the GNU Make oracle and only checks Rumk.
    minimum_make: Option<(u32, u32)>,
    /// The line GNU Make reports. Rejected fixtures only.
    line: Option<usize>,
    /// The line MK006 reports when it differs from GNU Make's: a missing
    /// `endif` is reported where the conditional opens, while GNU Make
    /// reports it at the end of the file.
    rumk_line: Option<usize>,
    /// Text the MK006 message must contain. Rejected fixtures only.
    message: Option<String>,
    /// Whether the GNU Make oracle runs the recipes of a rejected fixture
    /// instead of printing them. A failure that only happens once a command
    /// runs, such as expanding an exported variable, needs this.
    run_recipes: bool,
}

impl SyntaxFixture {
    fn load(path: &Path) -> Self {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // A fixture is read the way Rumk reads a Makefile, so a byte order
        // mark stands only in the bytes GNU Make is given.
        let bytes = std::fs::read(path).unwrap();
        let source = String::from_utf8(rumk::source::split_byte_order_mark(&bytes).0.to_vec())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut fixture = Self {
            path: path.to_path_buf(),
            name,
            source: String::new(),
            minimum_make: None,
            line: None,
            rumk_line: None,
            message: None,
            run_recipes: false,
        };
        for line in source.lines() {
            let Some((key, value)) = line
                .strip_prefix("# ")
                .and_then(|header| header.split_once(": "))
            else {
                break;
            };
            let name = &fixture.name;
            match key {
                "make" => {
                    fixture.minimum_make = value
                        .strip_prefix(">=")
                        .and_then(parse_make_version)
                        .or_else(|| panic!("{name}: `# make:` takes `>=MAJOR.MINOR`"));
                }
                "line" => {
                    fixture.line = Some(
                        value
                            .parse()
                            .unwrap_or_else(|_| panic!("{name}: `# line:` takes a line number")),
                    );
                }
                "rumk-line" => {
                    fixture.rumk_line =
                        Some(value.parse().unwrap_or_else(|_| {
                            panic!("{name}: `# rumk-line:` takes a line number")
                        }));
                }
                "rumk" => fixture.message = Some(value.to_string()),
                "run-recipes" => {
                    fixture.run_recipes = match value {
                        "true" => true,
                        "false" => false,
                        _ => panic!("{name}: `# run-recipes:` takes `true` or `false`"),
                    };
                }
                _ => panic!("{name}: unknown fixture header `{key}`"),
            }
        }
        fixture.source = source;
        fixture
    }
}

fn syntax_fixtures(group: &str) -> Vec<SyntaxFixture> {
    let directory = fixture("syntax").join(group);
    let mut fixtures: Vec<_> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "mk"))
        .map(|path| SyntaxFixture::load(&path))
        .collect();
    assert!(
        !fixtures.is_empty(),
        "no fixtures in {}",
        directory.display()
    );
    fixtures.sort_by(|left, right| left.name.cmp(&right.name));
    fixtures
}

fn parse_make_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// The GNU Make on `PATH` as (major, minor), or `None` when there is none.
fn installed_make_version() -> Option<(u32, u32)> {
    let output = match Command::new("make").arg("--version").output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("failed to launch GNU Make: {error}"),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let banner = stdout.lines().next().unwrap_or_default();
    let version = banner
        .strip_prefix("GNU Make ")
        .and_then(parse_make_version);
    if version.is_none() {
        eprintln!("skipping the GNU Make oracle: `make --version` reported {banner:?}");
    }
    version
}

/// Whether the installed GNU Make can serve as the oracle for `fixture`.
fn make_oracle_applies(fixture: &SyntaxFixture, installed: Option<(u32, u32)>) -> bool {
    let Some((major, minor)) = installed else {
        return false;
    };
    match fixture.minimum_make {
        Some((required_major, required_minor))
            if (major, minor) < (required_major, required_minor) =>
        {
            eprintln!(
                "{}: skipping the GNU Make oracle, {major}.{minor} is older than {required_major}.{required_minor}",
                fixture.name
            );
            false
        }
        _ => true,
    }
}

fn run_make(fixture: &SyntaxFixture, arguments: &[&str]) -> Output {
    Command::new("make")
        .current_dir(fixture.path.parent().unwrap())
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to launch GNU Make: {error}"))
}

fn mk006_diagnostics(fixture: &SyntaxFixture) -> Vec<(usize, usize, String)> {
    InvalidSyntax
        .check(&parse(&fixture.source), &fixture.source)
        .into_iter()
        .map(|diagnostic| (diagnostic.line, diagnostic.column, diagnostic.message))
        .collect()
}

#[test]
fn rejected_syntax_fixtures_fail_in_gnu_make_where_mk006_reports_them() {
    let installed = installed_make_version();
    for fixture in syntax_fixtures("rejected") {
        let name = &fixture.name;
        let line = fixture
            .line
            .unwrap_or_else(|| panic!("{name}: rejected fixtures declare `# line: N`"));
        let message = fixture
            .message
            .as_deref()
            .unwrap_or_else(|| panic!("{name}: rejected fixtures declare `# rumk: <message>`"));

        let diagnostics = mk006_diagnostics(&fixture);
        let (reported_line, _, reported_message) = diagnostics
            .first()
            .unwrap_or_else(|| panic!("{name}: MK006 reported nothing"));
        assert_eq!(
            *reported_line,
            fixture.rumk_line.unwrap_or(line),
            "{name}: MK006 reported {reported_message:?} at line {reported_line}"
        );
        assert!(
            reported_message.contains(message),
            "{name}: MK006 reported {reported_message:?}, expected {message:?}"
        );

        if make_oracle_applies(&fixture, installed) {
            let mut arguments = vec!["--no-builtin-rules"];
            if !fixture.run_recipes {
                arguments.push("--dry-run");
            }
            arguments.extend(["-f", name]);
            let output = run_make(&fixture, &arguments);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !output.status.success(),
                "{name}: GNU Make accepted the fixture"
            );
            assert!(
                stderr.contains(&format!("{name}:{line}:")),
                "{name}: GNU Make did not fail at line {line}:\n{stderr}"
            );
        }
    }
}

#[test]
fn accepted_syntax_fixtures_run_in_gnu_make_and_are_clean_for_mk006() {
    let installed = installed_make_version();
    for fixture in syntax_fixtures("accepted") {
        let name = &fixture.name;
        assert!(
            fixture.line.is_none()
                && fixture.rumk_line.is_none()
                && fixture.message.is_none()
                && !fixture.run_recipes,
            "{name}: accepted fixtures carry no MK006 expectations and always run their recipes"
        );

        let diagnostics = mk006_diagnostics(&fixture);
        assert!(
            diagnostics.is_empty(),
            "{name}: MK006 reported {diagnostics:?}"
        );

        if make_oracle_applies(&fixture, installed) {
            let output = run_make(&fixture, &["--no-builtin-rules", "-f", name, "verify"]);
            assert!(
                output.status.success(),
                "{name}: GNU Make rejected the fixture:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
