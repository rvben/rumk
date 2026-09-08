#!/usr/bin/env python3
"""Benchmark pinned linters on clean real projects; never execute their Makefiles.

Includes CLI startup, output serialization, wall time, and per-process peak RSS.
Profiles intentionally differ: measurements describe these commands, not equal
analysis work. Reports and copied checkouts are local working material.
"""
import argparse
import hashlib
from functools import lru_cache
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import statistics
import subprocess
import tempfile
import time

SPEC = importlib.util.spec_from_file_location("comparison", Path(__file__).with_name("compare-linters.py"))
COMPARE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMPARE)


@lru_cache(maxsize=1)
def timer_options():
    system = platform.system()
    timer = Path("/usr/bin/time")
    if timer.exists() and system == "Darwin":
        options, pattern, multiplier = [str(timer), "-l"], r"(?m)^\s*(\d+)\s+maximum resident set size\s*$", 1
    elif timer.exists() and system == "Linux":
        options, pattern, multiplier = [str(timer), "-f", "RUMK_PEAK_RSS_KIB=%M"], r"(?m)^RUMK_PEAK_RSS_KIB=(\d+)\s*$", 1024
    else:
        return [], None, None
    # Sandboxes can deny macOS time's sysctl, making the wrapper overwrite the
    # child's exit status. Probe before measuring, never rerun a measured child.
    probe = subprocess.run([*options, "/usr/bin/true"], capture_output=True, text=True, timeout=5)
    if probe.returncode or not re.search(pattern, probe.stderr):
        return [], None, None
    return options, pattern, multiplier


def measured(command, directory, timeout):
    options, pattern, multiplier = timer_options()
    wrapped = [*options, *command]
    env = dict(os.environ, LC_ALL="C", NO_COLOR="1", TERM="dumb", COLUMNS="120", PYTHONHASHSEED="0")
    for name in ("MAKEFLAGS", "MFLAGS", "GNUMAKEFLAGS"):
        env.pop(name, None)
    start = time.perf_counter()
    process = subprocess.Popen(wrapped, cwd=directory, env=env, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, start_new_session=os.name == "posix")
    timed_out = False
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        stdout, stderr = process.communicate()
        timed_out = True
    elapsed = time.perf_counter() - start
    stderr = stderr.decode("utf-8", errors="replace")
    match = re.search(pattern, stderr) if pattern else None
    # Keep the exact tool output and timer output for inspection. Timer stderr
    # is deliberately not interpreted as a linter diagnostic.
    return {"seconds": elapsed, "peak_rss_bytes": int(match[1]) * multiplier if match else None,
            "exit_code": None if timed_out else process.returncode, "timed_out": timed_out,
            "stdout": stdout.decode("utf-8", errors="replace").replace(str(directory), "<project>"),
            "stderr_with_measurement": stderr.replace(str(directory), "<project>")}


def summary(samples):
    memory = [s["peak_rss_bytes"] for s in samples if s["peak_rss_bytes"] is not None]
    timeouts = sum(s.get("timed_out", False) for s in samples)
    return {"median_seconds": None if timeouts else statistics.median(s["seconds"] for s in samples),
            "median_peak_rss_bytes": statistics.median(memory) if not timeouts and len(memory) == len(samples) else None,
            "timed_out_samples": timeouts,
            "exit_codes": sorted(set(s["exit_code"] for s in samples if s["exit_code"] is not None)), "samples": samples}


def stable_stdout(tool, output):
    if tool == "checkmake" and output.strip():
        return json.dumps(sorted(json.loads(output), key=lambda d: json.dumps(d, sort_keys=True)), sort_keys=True)
    return output


def benchmark(binaries, projects, manifest, runs, warmups, timeout):
    report = {"schema_version": 1, "platform": platform.platform(), "runs": runs, "warmups": warmups,
              "method": "Clean pinned tracked projects copied to disposable directories. Static check commands only; no Make invocation. Alternating tool order. Wall time and peak RSS include process startup and a system time wrapper. Warm filesystem caches; unequal rule sets and dialects. Nonzero statuses are retained, not scored as defects. RSS is null where unavailable.",
              "tools": {}, "projects": {}, "startup": {}, "failures": [], "resource_limits": []}
    for tool, binary in binaries.items():
        version = COMPARE.run([binary, "--version"], Path.cwd(), timeout)
        output = version["stdout"] + version["stderr"]
        if version["exit_code"] or (tool in COMPARE.PINS and not re.search(r"(?<![\d.])" + re.escape(COMPARE.PINS[tool]) + r"(?![\d.])", output)):
            raise ValueError(f"Unexpected {tool} version: {output}")
        report["tools"][tool] = {"version": output.strip(), "executable_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest()}
        samples = [measured([binary, "--version"], Path.cwd(), timeout) for _ in range(runs)]
        report["startup"][tool] = summary(samples)
    for project in projects:
        revision = subprocess.check_output(["git", "-C", str(project), "rev-parse", "HEAD"], text=True).strip()
        if revision != manifest[project.name]["revision"]:
            raise ValueError(f"Revision differs from manifest: {project.name}")
        if subprocess.check_output(["git", "-C", str(project), "status", "--porcelain", "--untracked-files=no"]):
            raise ValueError(f"Tracked changes in {project}")
        names = subprocess.check_output(["git", "-C", str(project), "ls-files", "-z"]).decode().split("\0")
        with tempfile.TemporaryDirectory(prefix="rumk-linter-bench-") as temp:
            directory = Path(temp)
            for name in filter(None, names):
                source, destination = project / name, directory / name
                if source.is_file() and not source.is_symlink():
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, destination)
                elif source.is_symlink():
                    target = source.resolve()
                    if not target.is_relative_to(project):
                        raise ValueError(f"Symlink leaves project: {project.name}/{name}")
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.symlink_to(os.path.relpath(directory / target.relative_to(project), destination.parent))
            entry = next((name for name in ("GNUmakefile", "makefile", "Makefile") if (directory / name).is_file()), None)
            if entry is None:
                raise ValueError(f"No root Makefile in {project}")
            config_dir = directory / ".rumk-benchmark-settings"
            config_dir.mkdir()
            (config_dir / "checkmake.ini").write_text("[minphony]\nrequired =\n")
            (config_dir / "bake.toml").write_text("[formatter]\n")
            before = COMPARE.snapshot(directory)
            commands = {
                "rumk": ["--no-config", "check", "--output-format", "json", entry],
                "checkmake": ["--config", str(config_dir / "checkmake.ini"), "--output", "json", entry],
                "unmake": [entry],
                "mbake": ["format", "--check", "--config", str(config_dir / "bake.toml"), entry],
            }
            records = {tool: [] for tool in binaries}
            observations = {}
            timed_out_tools = set()
            for iteration in range(runs + warmups):
                for tool in list(binaries)[::1 if iteration % 2 == 0 else -1]:
                    if tool in timed_out_tools:
                        continue
                    result = measured([binaries[tool], *commands[tool]], directory, timeout)
                    if COMPARE.snapshot(directory) != before:
                        raise RuntimeError(f"{tool} mutated {project.name} during checking")
                    if result["timed_out"]:
                        timed_out_tools.add(tool)
                        records[tool].append(result)
                        report["resource_limits"].append(f"{project.name}/{tool} exceeded {timeout}s; remaining repetitions skipped")
                        continue
                    observation = (result["exit_code"], stable_stdout(tool, result["stdout"]))
                    if tool in observations and observation != observations[tool]:
                        report["failures"].append(f"Unstable check output: {project.name}/{tool}")
                    observations[tool] = observation
                    if iteration >= warmups:
                        records[tool].append(result)
            print(f"Measured {project.name}; timed out: {', '.join(sorted(timed_out_tools)) or 'none'}", flush=True)
            report["projects"][project.name] = {"revision": revision, "inputs": before, "entry": entry,
                "tools": {tool: dict(summary(samples), command=[tool, *(arg.replace(str(directory), "<project>") for arg in commands[tool])]) for tool, samples in records.items()}}
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rumk", type=Path, default=Path("target/release/rumk"))
    for tool in COMPARE.PINS:
        parser.add_argument("--" + tool, type=Path, required=True)
    parser.add_argument("--project", type=Path, action="append", required=True)
    parser.add_argument("--manifest", type=Path, default=Path("scripts/corpus-projects.json"))
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=30)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.runs < 1 or args.warmups < 0 or args.timeout <= 0:
        parser.error("runs/timeout must be positive and warmups nonnegative")
    projects = [p.resolve() for p in args.project]
    if len({p.name for p in projects}) != len(projects):
        parser.error("Project names must be unique")
    report = benchmark({tool: str(getattr(args, tool).resolve()) for tool in ("rumk", *COMPARE.PINS)}, projects,
                       json.loads(args.manifest.read_text()), args.runs, args.warmups, args.timeout)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    for project, record in report["projects"].items():
        for tool, result in record["tools"].items():
            timing = "timeout (no median)" if result["median_seconds"] is None else f"{result['median_seconds']*1000:.2f} ms"
            print(f"{project}/{tool}: {timing}, RSS {result['median_peak_rss_bytes']} bytes, exits {result['exit_codes']}")
    return bool(report["failures"])


if __name__ == "__main__":
    raise SystemExit(main())
