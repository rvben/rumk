#!/usr/bin/env python3
"""Exercise the actual Rust pre-commit hooks in disposable repositories.

Run with the Python environment containing pre-commit (tested with 4.6.0).
No hooks, commits, or index changes are made in the working repository.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

import yaml
from pre_commit.clientlib import load_manifest

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pre-commit", default="pre-commit")
    args = parser.parse_args()
    hooks = load_manifest(str(ROOT / ".pre-commit-hooks.yaml"))
    with tempfile.TemporaryDirectory(prefix="rumk-hooks-") as temp:
        work = Path(temp)
        env = dict(os.environ, PRE_COMMIT_HOME=str(work / "cache"), CARGO_NET_OFFLINE="true")
        def run(command, cwd, expected=0):
            result = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True, timeout=600)
            if result.returncode != expected:
                raise AssertionError(f'{command}: expected {expected}, got {result.returncode}\n{result.stdout}\n{result.stderr}')
            return result.stdout + result.stderr
        source = work / "source"
        source.mkdir()
        for name in ("Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "rumk.schema.json", ".pre-commit-hooks.yaml"):
            shutil.copy2(ROOT / name, source / name)
        for name in ("src",):
            shutil.copytree(ROOT / name, source / name)
        run(["git", "init", "-q"], source)
        run(["git", "add", "Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "rumk.schema.json", ".pre-commit-hooks.yaml", "src"], source)
        staged = run(["git", "diff", "--cached", "--name-only"], source).splitlines()
        allowed = {"Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "rumk.schema.json", ".pre-commit-hooks.yaml"}
        assert staged and all(name in allowed or (name.startswith("src/") and name.endswith(".rs")) for name in staged)
        run(["git", "-c", "user.name=Hook Test", "-c", "user.email=hook-test@example.invalid",
             "commit", "-qm", "test: exercise local pre-commit hooks"], source)
        revision = run(["git", "rev-parse", "HEAD"], source).strip()
        project = work / "project"
        project.mkdir()
        run(["git", "init", "-q"], project)
        (project / ".pre-commit-config.yaml").write_text(yaml.safe_dump({"repos": [{
            "repo": str(source), "rev": revision, "hooks": [{"id": "rumk-fmt"}, {"id": "rumk-check"}]}]}))
        (project / ".rumk.toml").write_text('[MK105]\nenabled = true\n[MK215]\nenabled = true\nrequired = ["test"]\n')
        (project / "Makefile").write_text('include tasks.mk\n.PHONY: test\n')
        (project / "tasks.mk").write_text('test:;\n')
        (project / "space dir").mkdir()
        (project / "space dir" / "extra.mk").write_text('VALUE=text  \n')
        run(["git", "add", "."], project)
        base = [args.pre_commit, "run", "--all-files"]
        output = run(base, project, 1)
        assert 'files were modified' in output, output
        assert (project / "space dir" / "extra.mk").read_text() == 'VALUE = text  \n'
        run(["git", "add", "."], project)
        run(base, project)
        # Only the included fragment changes; check must still use the root.
        (project / "tasks.mk").write_text('test:;\n.IGNORE:\n')
        output = run([args.pre_commit, "run", "rumk-check", "--files", "tasks.mk"], project, 1)
        assert 'MK214' in output, output
        # Configuration-only changes must trigger checking as well.
        (project / "tasks.mk").write_text('test:;\n')
        (project / ".rumk.toml").write_text('[MK215]\nenabled = true\nrequired = ["missing"]\n')
        output = run([args.pre_commit, "run", "rumk-check", "--files", ".rumk.toml"], project, 1)
        assert 'MK215' in output, output
        (project / "notes.txt").write_text('not a makefile\n')
        run(["git", "add", "notes.txt"], project)
        output = run([args.pre_commit, "run", "--files", "notes.txt"], project)
        assert 'Skipped' in output, output
        assert {hook['id'] for hook in hooks} == {'rumk-check', 'rumk-fmt'}
    print('Rust hook installation, formatting, idempotence, include context, config triggers, and file filtering passed.')


if __name__ == "__main__":
    main()
