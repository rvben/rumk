#!/usr/bin/env python3
"""Measure paired, authored defects against GNU Make behavior, not linter votes.

Python 3.9+. Executes ONLY the trusted fixtures in tests/fixtures/semantic.
Does not download tools or execute upstream corpus Makefiles.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "tests/fixtures/semantic/manifest.json"
SPEC = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("compare-linters.py"))
COMPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARE)


def invoke(command, directory, timeout=20):
    # Host make variables must not change the ground truth or linter profile.
    env = {key: os.environ[key] for key in ("PATH", "SystemRoot") if key in os.environ}
    env.update(HOME=str(directory), LC_ALL="C", NO_COLOR="1", TERM="dumb",
               COLUMNS="120", PYTHONHASHSEED="0")
    start = time.perf_counter()
    result = subprocess.run(command, cwd=directory, env=env, capture_output=True,
                            text=True, timeout=timeout)
    return {"exit_code": result.returncode,
            "stdout": result.stdout.replace(str(directory), "<fixture>"),
            "stderr": result.stderr.replace(str(directory), "<fixture>"),
            "seconds": time.perf_counter() - start}


def populate(directory, case, variant, command_targets=(), external_variables=()):
    files = dict(case["files"], Makefile=case[variant])
    if variant == "working":
        files.update(case.get("working_files", {}))
    files.update({"bake.toml": "[formatter]\n", "checkmake.ini": "[minphony]\nrequired =\n"})
    files["rumk.toml"] = ('[MK201]\ncommand-targets = ' + json.dumps(list(command_targets)) + '\n'
                          + '[MK208]\nexternal-variables = ' + json.dumps(list(external_variables)) + '\n')
    for name, content in files.items():
        path = Path(name)
        if path.is_absolute() or ".." in path.parts:
            raise ValueError(f"Unsafe fixture path: {name}")
        path = directory / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")


def contract_matches(result, contract):
    for key, expected in contract.items():
        if key.endswith("_contains"):
            if expected not in result[key.removesuffix("_contains")]:
                return False
        elif key.endswith("_absent"):
            if expected in result[key.removesuffix("_absent")]:
                return False
        elif result[key] != expected:
            return False
    return True


def oracle(make, directory, case):
    return invoke([make, "--no-print-directory", *([] if case.get("builtin_rules") else ["-rR"]), "-f", "Makefile",
                   "MAKE=" + make, *case["args"]], directory)


def diagnostic_match(tool, result, case):
    """Match the named defect, never merely a warning or nonzero exit status."""
    if tool == "rumk":
        findings = json.loads(result["stdout"])
        return any(d["rule"] == case["rule"] and d["file"] == "Makefile"
                   for d in findings)
    if tool == "checkmake":
        findings = json.loads(result["stdout"]) if result["stdout"].strip() else []
        return any(d["rule"] == case["matchers"].get(tool)
                   and d["file_name"] == "Makefile" for d in findings)
    pattern = case["matchers"].get(tool)
    return bool(pattern and re.search(pattern, result["stdout"] + result["stderr"]))


def commands(tool, binary):
    return {
        # A single profile for every case; MK208, MK216, and MK217 are added opt-in rules.
        "rumk": [binary, "--config", "rumk.toml", "check", "--extend-enable", "MK208,MK216,MK217",
                 "--output-format", "json", "Makefile"],
        "checkmake": [binary, "--config", "checkmake.ini", "--output", "json", "Makefile"],
        "unmake": [binary, "Makefile"],
        "mbake": [binary, "format", "--check", "--config", "bake.toml", "Makefile"],
    }[tool]


def identity(binary):
    version = invoke([binary, "--version"], ROOT)
    if version["exit_code"]:
        raise RuntimeError(f"Version check failed: {binary}")
    return {"version": (version["stdout"] + version["stderr"]).strip(),
            "sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest()}


def diagnostic_exit(tool, result):
    # mbake uses exit 2 for source validation errors, including duplicate
    # recipes. Keep that finding; it is not an executable crash.
    return result["exit_code"] in (0, 1) or (tool == "mbake" and
        result["exit_code"] == 2 and re.search(r"Makefile:\d+: Error:", result["stdout"]) is not None)


def safe_fix(tool, binary, make, case, variant, command_targets=(), external_variables=()):
    if tool not in ("rumk", "mbake"):
        return {"available": False}
    with tempfile.TemporaryDirectory(prefix="rumk-semantic-fix-") as temp:
        directory = Path(temp)
        populate(directory, case, variant, command_targets, external_variables)
        before = COMPARE.snapshot(directory)
        command = (commands(tool, binary) + ["--fix"] if tool == "rumk" else
                   [binary, "format", "--config", "bake.toml", "Makefile"])
        first = invoke(command, directory)
        after = COMPARE.snapshot(directory)
        second = invoke(command, directory)
        idempotent = COMPARE.snapshot(directory) == after
        behavior = oracle(make, directory, case)
        return {"available": True, "command": [tool, *command[1:]],
                "changed": before != after, "inputs_after": after,
                "first": first, "second": second, "idempotent": idempotent,
                "behavior": behavior,
                "original_contract_preserved": contract_matches(behavior, case["oracle"][variant]),
                "working_contract_met": contract_matches(behavior, case["oracle"]["working"])}


def benchmark(binaries, make, runs, command_targets=(), external_variables=()):
    raw = MANIFEST.read_bytes()
    cases = json.loads(raw)["cases"]
    report = {"schema_version": 1, "manifest_sha256": hashlib.sha256(raw).hexdigest(),
              "platform": platform.platform(), "runs": runs, "warmups": 1,
              "tools": {tool: identity(binary) for tool, binary in binaries.items()},
              "gnu_make": identity(make), "results": [], "failures": [],
              "command_targets": list(command_targets), "external_variables": list(external_variables),
              "skipped": sorted(set(COMPARE.PINS) - set(binaries)),
              "method": "Authored paired GNU defects; one fixed rumk defaults+MK208+MK216 profile. Named-defect matching only; unmatched warnings are unscored. No population precision/recall or overall ranking. Checkmake has no required-target policy. Timings include startup; alternating order; fresh copies; warm cache. Fixes use default safe mode (mbake: formatter). Executable hashes do not pin interpreter dependencies."}
    if "GNU Make" not in report["gnu_make"]["version"]:
        raise ValueError("The semantic oracle requires GNU Make")
    for tool, pin in COMPARE.PINS.items():
        if tool in binaries and not re.search(r"(?<![\d.])v?" + re.escape(pin) + r"(?![\d.])",
                                              report["tools"][tool]["version"]):
            raise ValueError(f"Expected {tool} {pin}")
    for case in cases:
        row = {"name": case["name"], "title": case["title"], "rule": case["rule"],
               "oracle": {}, "tools": {tool: {} for tool in binaries}}
        for variant in ("broken", "working"):
            with tempfile.TemporaryDirectory(prefix="rumk-semantic-oracle-") as temp:
                directory = Path(temp)
                populate(directory, case, variant)
                result = oracle(make, directory, case)
                row["oracle"][variant] = result
                if not contract_matches(result, case["oracle"][variant]):
                    report["failures"].append(f'{case["name"]}/{variant}: GNU contract mismatch')
            samples = {tool: [] for tool in binaries}
            observations = {}
            for iteration in range(runs + 1):
                order = list(binaries)
                if iteration % 2:
                    order.reverse()
                for tool in order:
                    with tempfile.TemporaryDirectory(prefix="rumk-semantic-lint-") as temp:
                        directory = Path(temp)
                        populate(directory, case, variant, command_targets, external_variables)
                        before = COMPARE.snapshot(directory)
                        command = commands(tool, binaries[tool])
                        result = invoke(command, directory)
                        stable = COMPARE.observation(dict(result, case=case["name"], tool=tool, inputs=before))
                        if tool in observations and stable != observations[tool]:
                            report["failures"].append(f'{case["name"]}/{variant}/{tool}: unstable output')
                        observations[tool] = stable
                        if not diagnostic_exit(tool, result) or COMPARE.snapshot(directory) != before:
                            report["failures"].append(f'{case["name"]}/{variant}/{tool}: tool error or input mutation')
                        if iteration:
                            samples[tool].append(result["seconds"])
                        row["tools"][tool][variant] = dict(result, inputs=before,
                            command=[tool, *command[1:]], detected=diagnostic_match(tool, result, case))
            for tool in binaries:
                record = row["tools"][tool][variant]
                record.update(seconds=samples[tool], median_seconds=statistics.median(samples[tool]))
                record["safe_fix"] = safe_fix(tool, binaries[tool], make, case, variant, command_targets, external_variables)
                fix = record["safe_fix"]
                if fix["available"] and (not fix["idempotent"] or
                    any(not diagnostic_exit(tool, fix[step]) for step in ("first", "second"))):
                    report["failures"].append(f'{case["name"]}/{variant}/{tool}: fix error or not idempotent')
                if variant == "working" and fix["available"] and not fix["working_contract_met"]:
                    report["failures"].append(f'{case["name"]}/{tool}: formatter broke working control')
        report["results"].append(row)
    report["summary"] = {tool: {
        "defects_detected": sum(r["tools"][tool]["broken"]["detected"] for r in report["results"]),
        "defects_total": len(cases),
        "controls_flagged_for_named_defect": sum(r["tools"][tool]["working"]["detected"] for r in report["results"]),
        "controls_total": len(cases),
        "median_invocation_ms": statistics.median(v["median_seconds"] * 1000
            for r in report["results"] for v in r["tools"][tool].values()),
    } for tool in binaries}
    for tool, totals in report["summary"].items():
        detected = totals["defects_detected"]
        false_alarms = totals["controls_flagged_for_named_defect"]
        totals["paired_recall"] = detected / len(cases) if cases else None
        totals["paired_precision"] = detected / (detected + false_alarms) if detected + false_alarms else None
        totals["by_rule"] = {}
        for rule in sorted({case["rule"] for case in cases}):
            rows = [row for row in report["results"] if row["rule"] == rule]
            totals["by_rule"][rule] = {
                "defects": len(rows),
                "detected": sum(row["tools"][tool]["broken"]["detected"] for row in rows),
                "controls_flagged": sum(row["tools"][tool]["working"]["detected"] for row in rows),
            }
    report["known_rumk_coverage_gaps"] = [case["name"] for case in cases if not case["expected_rumk_detection"]]
    return report


def stable_report(value, tool=None):
    """Ignore measurements, preserving identities, fixtures, fixes and findings."""
    if isinstance(value, list):
        return [stable_report(item, tool) for item in value]
    if not isinstance(value, dict):
        return value
    result = {}
    for key, item in value.items():
        if key in ("seconds", "median_seconds", "median_invocation_ms"):
            continue
        if key == "stdout" and tool == "checkmake" and item.strip():
            item = json.dumps(sorted(json.loads(item), key=lambda d: json.dumps(d, sort_keys=True)), sort_keys=True)
        result[key] = stable_report(item, key if key in ("rumk", *COMPARE.PINS) else tool)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rumk", default=str(ROOT / "target/release/rumk"))
    parser.add_argument("--make", default=shutil.which("make"))
    for tool in COMPARE.PINS:
        parser.add_argument("--" + tool)
    parser.add_argument("--require-all", action="store_true")
    parser.add_argument("--command-target", action="append", default=[],
                        help="Declare project command intent for every rumk case; reported separately from defaults")
    parser.add_argument("--external-variable", action="append", default=[],
                        help="Declare a name-only MK208 input for every case; report separately")
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, help="Require identical identities and observations, excluding timings")
    args = parser.parse_args()
    if args.runs < 1 or not args.make:
        parser.error("Positive runs and a GNU Make executable are required")
    if args.require_all and any(not getattr(args, tool) for tool in COMPARE.PINS):
        parser.error("--require-all needs every competitor executable")
    binaries = {tool: str(Path(getattr(args, tool)).resolve())
                for tool in ("rumk", *COMPARE.PINS) if getattr(args, tool)}
    report = benchmark(binaries, str(Path(args.make).resolve()), args.runs, args.command_target, args.external_variable)
    if args.baseline and stable_report(report) != stable_report(json.loads(args.baseline.read_text())):
        report["failures"].append("Baseline identities or observations differ")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["summary"], indent=2))
    for failure in report["failures"]:
        print(failure)
    return bool(report["failures"])


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        raise SystemExit(f"Semantic benchmark could not complete: {error}") from error
