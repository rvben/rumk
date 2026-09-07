#!/usr/bin/env python3
"""Audit local Git checkouts without executing Make or modifying project files.

Python 3.9+. Checks disk/stdin diagnostic parity, repeatability, formatter
idempotence, and input integrity. Timings are observations, not speed rankings.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True,
                          capture_output=True).stdout


def inventory(root):
    """Include untracked files, which would expose accidental Make side effects."""
    result = {}
    for directory, directories, files in os.walk(root, followlinks=False):
        directories[:] = sorted(name for name in directories if name != ".git")
        for name in sorted(files + [name for name in directories
                                    if (Path(directory) / name).is_symlink()]):
            path = Path(directory) / name
            key = path.relative_to(root).as_posix()
            result[key] = ("symlink:" + os.readlink(path) if path.is_symlink()
                           else digest(path.read_bytes()))
    return result


def makefiles(root):
    names = git(root, "ls-files", "-z").decode("utf-8").split("\0")
    return sorted(name for name in names if name and
                  (Path(name).name in ("Makefile", "makefile", "GNUmakefile")
                   or Path(name).suffix in (".mk", ".make"))
                  and not (root / name).is_symlink())


def invoke(binary, root, args, timeout, data=None):
    env = dict(os.environ, NO_COLOR="1", TERM="dumb", LC_ALL="C")
    for name in ("MAKEFLAGS", "MFLAGS", "GNUMAKEFLAGS"):
        env.pop(name, None)
    start = time.perf_counter()
    result = subprocess.run([str(binary), "--no-config", *args], cwd=root,
                            input=data, capture_output=True, timeout=timeout, env=env)
    elapsed = time.perf_counter() - start
    if result.returncode not in (0, 1):
        raise RuntimeError(f'{args}: exit {result.returncode}: {result.stderr.decode(errors="replace")}')
    return result, elapsed


def normalized(result, root):
    return {"exit_code": result.returncode,
            "stdout": result.stdout.decode().replace(str(root), "<project>"),
            "stderr": result.stderr.decode().replace(str(root), "<project>")}


def audit(binary, root, runs, timeout):
    if git(root, "status", "--porcelain", "--untracked-files=no").strip():
        raise RuntimeError(f"Tracked files must be clean: {root}")
    revision = git(root, "rev-parse", "HEAD").decode().strip()
    before = inventory(root)
    paths = makefiles(root)
    if not paths:
        raise RuntimeError(f"No tracked Makefiles: {root}")
    observations = []
    failures = []
    for path in paths:
        data = (root / path).read_bytes()
        samples = []
        expected = None
        for iteration in range(runs + 1):  # one unmeasured warm-up
            result, elapsed = invoke(binary, root, ["check", path, "--output-format", "json"], timeout)
            value = normalized(result, root)
            json.loads(value["stdout"])
            if expected is not None and value != expected:
                failures.append(f"{path}: unstable diagnostics")
            expected = value
            if iteration:
                samples.append(elapsed)
        buffer, _ = invoke(binary, root, ["check", "-", "--stdin-filename", path,
                                         "--output-format", "json"], timeout, data)
        if normalized(buffer, root) != expected:
            failures.append(f"{path}: disk/stdin diagnostics differ")
        format_args = ["fmt", "-", "--stdin-filename", path, "--extend-enable", "MK105"]
        formatted, _ = invoke(binary, root, format_args, timeout, data)
        repeated, _ = invoke(binary, root, format_args, timeout, formatted.stdout)
        if formatted.returncode or repeated.returncode or formatted.stderr or repeated.stderr:
            failures.append(f"{path}: formatting failed")
        if repeated.stdout != formatted.stdout:
            failures.append(f"{path}: formatting is not idempotent")
        observations.append({"file": path, "input_sha256": digest(data),
                             "check": expected, "seconds": samples,
                             "median_seconds": statistics.median(samples),
                             "formatted_sha256": digest(formatted.stdout),
                             "format_changed": formatted.stdout != data})
    if inventory(root) != before:
        failures.append("Project files changed during read-only audit")
    return {"revision": revision, "inputs": before, "files": observations, "failures": failures}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/rumk"))
    parser.add_argument("--project", type=Path, action="append", required=True)
    parser.add_argument("--manifest", type=Path, help="Require exactly the project names and revisions in a pinned manifest")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.runs < 1 or args.timeout <= 0:
        parser.error("runs and timeout must be positive")
    roots = [path.resolve() for path in args.project]
    if len({root.name for root in roots}) != len(roots):
        parser.error("project directory names must be unique")
    manifest = json.loads(args.manifest.read_text()) if args.manifest else None
    if manifest is not None:
        if set(manifest) != {root.name for root in roots}:
            parser.error("project names must exactly match the manifest")
        for root in roots:
            revision = git(root, "rev-parse", "HEAD").decode().strip()
            if revision != manifest[root.name]["revision"]:
                parser.error(f"{root.name}: checkout revision differs from manifest")
    binary = args.binary.resolve()
    report = {"schema_version": 1, "binary_sha256": digest(binary.read_bytes()),
              "platform": platform.platform(), "runs": args.runs,
              "method": "Clean tracked Makefiles; built-in checking defaults; formatting adds MK105; one warm-up; no Make execution. Diagnostics are unreviewed observations, not confirmed defects.",
              "projects": {}}
    if manifest is not None:
        report["manifest"] = manifest
    for root in roots:
        value = audit(binary, root, args.runs, args.timeout)
        report["projects"][root.name] = value
        print(f'{root.name}: {len(value["files"])} files, {len(value["failures"])} failures', flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    return any(project["failures"] for project in report["projects"].values())


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(f"Corpus audit could not complete: {error}") from error
