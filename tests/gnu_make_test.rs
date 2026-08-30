use std::path::PathBuf;
use std::process::Command;

use rumk::parser::{parse, VariableScope};
use rumk::project::{Project, ProjectOptions};

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
fn advanced_fixture_has_the_expected_rumk_structure() {
    let path = fixture("advanced.mk");
    let source = std::fs::read_to_string(path).unwrap();
    let makefile = parse(&source).unwrap();

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
    let all = project.analysis().target("all").unwrap();
    assert_eq!(
        all.dependencies
            .iter()
            .map(|dependency| dependency.prerequisite.as_str())
            .collect::<Vec<_>>(),
        ["one", "two"]
    );
}
