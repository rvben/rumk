#!/usr/bin/env python3
"""Run pinned linters on the authored comparison corpus, without downloading tools.

Python 3.9+. Every invocation uses a fresh temporary copy. Only rumk's explicit
expectations are assertions; competitor diagnostics are observations, not votes
on correctness. --baseline detects changes in those observations on later runs.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "tests/fixtures/comparison"
PINS = {"checkmake": "0.3.2", "unmake": "0.0.27", "mbake": "1.4.6"}


def run(command, cwd, timeout):
    env = dict(os.environ, NO_COLOR="1", TERM="dumb", COLUMNS="120", LC_ALL="C", PYTHONHASHSEED="0")
    for name in ("MAKEFLAGS", "MFLAGS", "GNUMAKEFLAGS"):
        env.pop(name, None)
    result = subprocess.run(command, cwd=cwd, env=env, capture_output=True,
                            text=True, timeout=timeout, check=False)
    return {"exit_code": result.returncode,
            "stdout": result.stdout.replace(str(cwd), "<fixture>"),
            "stderr": result.stderr.replace(str(cwd), "<fixture>")}


def snapshot(directory):
    return {str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(directory.rglob("*")) if path.is_file()}


def observation(result):
    value = {key: result[key] for key in ("case", "tool", "exit_code", "stdout", "stderr", "inputs")}
    # checkmake findings at the same location have changed order across
    # repeated runs. Preserve raw output in the report, but
    # compare its complete diagnostic multiset rather than incidental ordering.
    if result["tool"] == "checkmake" and result["stdout"].strip():
        diagnostics = json.loads(result["stdout"])
        if not isinstance(diagnostics, list):
            raise ValueError("checkmake JSON output must be a diagnostic array")
        value["stdout"] = json.dumps(sorted(diagnostics, key=lambda item: json.dumps(item, sort_keys=True)), sort_keys=True)
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rumk", default=str(ROOT / "target/debug/rumk"))
    for tool in PINS:
        parser.add_argument("--" + tool, help="Path to the pinned executable (omit to skip)")
    parser.add_argument("--require-all", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--timeout", type=int, default=20)
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    if args.require_all and any(not getattr(args, tool) for tool in PINS):
        parser.error("--require-all needs --checkmake, --unmake, and --mbake")
    binaries = {"rumk": str(Path(args.rumk).resolve())}
    binaries.update({tool: str(Path(getattr(args, tool)).resolve())
                     for tool in PINS if getattr(args, tool)})
    manifest_bytes = (CORPUS / "manifest.json").read_bytes()
    manifest = json.loads(manifest_bytes)
    report = {"schema_version": 1,
              "method": "Authored probes; per-case rumk rules; checkmake required=test/maxBodyLength=2; unmake static default; mbake format --check with explicit defaults. No speed ranking.",
              "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
              "environment": {"NO_COLOR": "1", "TERM": "dumb", "COLUMNS": "120", "LC_ALL": "C", "PYTHONHASHSEED": "0"},
              "tools": {}, "skipped": sorted(set(PINS) - set(binaries)), "results": []}
    failures = []
    for tool, binary in binaries.items():
        version = run([binary, "--version"], ROOT, args.timeout)
        version_text = version["stdout"] + version["stderr"]
        if version["exit_code"] != 0:
            raise RuntimeError(f"{tool} version command failed: {version_text}")
        if tool in PINS and not re.search(r"(?<![\d.])v?" + re.escape(PINS[tool]) + r"(?![\d.])", version_text):
            raise RuntimeError(f"Expected {tool} {PINS[tool]}, received {version_text!r}")
        report["tools"][tool] = {"version": version_text.strip(),
                                "sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest()}
    for case in manifest["cases"]:
        for tool, binary in binaries.items():
            with tempfile.TemporaryDirectory(prefix="rumk-comparison-") as temp:
                directory = Path(temp)
                shutil.copytree(CORPUS / case["name"], directory, dirs_exist_ok=True)
                (directory / ".rumk.toml").write_text(
                    f'[global]\nenable = {json.dumps(case["enable"])}\n'
                    '[MK104]\nmax-lines = 2\n[MK215]\nrequired = ["test"]\n')
                (directory / "checkmake.ini").write_text('[minphony]\nrequired = test\n[maxbodylength]\nmaxBodyLength = 2\n')
                (directory / "bake.toml").write_text('[formatter]\n')
                commands = {
                    "rumk": [binary, "check", "--output-format", "json", case["entry"]],
                    "checkmake": [binary, "--config", "checkmake.ini", "--output", "json", case["entry"]],
                    "unmake": [binary, case["entry"]],
                    "mbake": [binary, "format", "--check", "--config", "bake.toml", case["entry"]],
                }
                before = snapshot(directory)
                result = run(commands[tool], directory, args.timeout)
                result.update(case=case["name"], tool=tool, category=case["category"],
                              dialect=case["dialect"], command=[tool] + commands[tool][1:], inputs=before)
                if snapshot(directory) != before:
                    failures.append(f'{case["name"]}/{tool} modified inputs in check mode')
                if result["exit_code"] not in (0, 1):
                    failures.append(f'{case["name"]}/{tool} tool error: {result["exit_code"]}')
                if tool == "rumk":
                    diagnostics = json.loads(result["stdout"])
                    actual = [{"rule": d["rule"], "line": d["line"], "file": d["file"].replace("\\", "/")}
                              for d in diagnostics]
                    passed = actual == case["expected"] and result["exit_code"] == int(bool(actual))
                    result["expectation_passed"] = passed
                    if not passed:
                        failures.append(f'{case["name"]}: expected {case["expected"]}, got {actual}')
                report["results"].append(result)
    if args.baseline:
        baseline = json.loads(args.baseline.read_text())
        if report["manifest_sha256"] != baseline["manifest_sha256"]:
            failures.append("Baseline corpus manifest differs")
        if report["tools"] != baseline["tools"]:
            failures.append("Baseline executable identities differ")
        if [observation(r) for r in report["results"]] != [observation(r) for r in baseline["results"]]:
            failures.append("Baseline observations differ")
    report["failures"] = failures
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f'{len(manifest["cases"])} cases × {len(binaries)} tools; {len(failures)} failures; {args.output}')
    for failure in failures:
        print(failure)
    return bool(failures)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        raise SystemExit(f"Comparison could not complete: {error}") from error
