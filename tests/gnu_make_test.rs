use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rumk::fix::apply_fixes;
use rumk::parser::{parse, VariableScope};
use rumk::project::{Project, ProjectOptions};
use rumk::rules::style::LineLength;
use rumk::rules::syntax::InvalidSyntax;
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
fn wrapped_phony_fix_is_accepted_by_gnu_make() {
    let content = concat!(
        ".PHONY: build test lint clean release\n",
        "build test lint clean release:\n",
        "\t@:\n",
    );
    let diagnostics = LineLength::new(32).check(&parse(content), content);
    let fixed = apply_fixes(content, &diagnostics);
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
        let source = std::fs::read_to_string(path).unwrap();
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
