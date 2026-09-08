use rumk::project::{Project, ProjectOptions};
use rumk::rules::{rebuild::PhonyPrerequisite, Rule};
use std::process::Command;

fn project(content: &str, includes: &[(&str, &str)]) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in includes {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let path = dir.path().join("Makefile");
    std::fs::write(&path, content).unwrap();
    let project = Project::load(&path, &ProjectOptions::default()).unwrap();
    (dir, project)
}

#[test]
fn phony_normal_edges_report_at_the_consumer_across_includes() {
    let (_, project) = project(
        "include build.mk\nNAME := setup\n.PHONY: $(NAME)\n",
        &[("build.mk", "# build\noutput: setup setup\n\t@echo build\n")],
    );
    let findings = PhonyPrerequisite.check_project(&project);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].line, 2);
    assert!(findings[0].source.as_ref().unwrap().ends_with("build.mk"));
    assert!(findings[0].message.contains("'output'"));
    assert!(findings[0].fix.is_none());
}

#[test]
fn phony_rebuild_check_respects_valid_lookalikes_and_uncertainty() {
    for text in [
        ".PHONY: setup\noutput: | setup\n\t@echo build\n",
        ".PHONY: output setup\noutput: setup\n\t@echo build\n",
        ".PHONY: setup\nall: setup\n",
        "setup:;\noutput: setup\n\t@echo build\n",
        ".PHONY: setup\n%.o: setup\n\t@echo build\n",
        "ifeq (yes,no)\n.PHONY: setup\nendif\noutput: setup\n\t@echo build\n",
        "ifdef UNKNOWN\n.PHONY: output\nendif\n.PHONY: setup\noutput: setup\n\t@echo build\n",
        "include absent.mk\n.PHONY: setup\noutput: setup\n\t@echo build\n",
        "-include generated.mk\n.PHONY: setup\noutput: setup\n\t@echo build\n",
        "$(eval .PHONY: output)\n.PHONY: setup\noutput: setup\n\t@echo build\n",
        ".PHONY: setup\noutput: X=yes\noutput: setup\n",
        ".PHONY: setup\noutput:: setup\noutput::\n\t@echo build\n",
        ".SECONDEXPANSION:\n.PHONY: setup\noutput: setup\n\t@echo build\n",
    ] {
        let (_, project) = project(text, &[]);
        assert!(
            PhonyPrerequisite.check_project(&project).is_empty(),
            "{text}"
        );
    }
}

#[test]
fn gnu_make_demonstrates_normal_versus_order_only_phony_rebuilds() {
    if Command::new("make").arg("--version").output().is_err() {
        return;
    }
    for (separator, builds, warnings) in [("", 2, 1), ("| ", 1, 0)] {
        let text = format!(".PHONY: setup\noutput: {separator}setup\n\t@echo build >> log\n\t@touch output\nsetup:;\n");
        let (dir, project) = project(&text, &[]);
        assert_eq!(PhonyPrerequisite.check_project(&project).len(), warnings);
        for _ in 0..2 {
            let output = Command::new("make")
                .current_dir(dir.path())
                .args(["-rR", "output"])
                .env_remove("MAKEFLAGS")
                .env_remove("MFLAGS")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_eq!(
            std::fs::read_to_string(dir.path().join("log"))
                .unwrap()
                .lines()
                .count(),
            builds
        );
    }
}
