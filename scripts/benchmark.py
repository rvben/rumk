#!/usr/bin/env python3
"""Reproducible CLI benchmarks; requires Python 3.9+ and a release binary."""

import argparse
import hashlib
import json
import platform
from pathlib import Path
import statistics
import subprocess
import tempfile
import time


def generate(root, scale):
    graph = root / "includes"
    graph.mkdir()
    # Stay below the default 1024-file project limit, including the root.
    count = min(1000, 100 * scale)
    (graph / "Makefile").write_text(
        "MODE ?= release\ninclude config.mk\n"
        + "".join(f"include part{i}.mk\n" for i in range(count))
        + "all: target0\n",
        encoding="utf-8",
    )
    (graph / "config.mk").write_text("FLAGS := -O2\n", encoding="utf-8")
    for i in range(count):
        (graph / f"part{i}.mk").write_text(
            f"VALUE_{i} := $(strip a b c)\n"
            f"ifeq ($(MODE),release)\ntarget{i}:\n\t@echo $(FLAGS)\nendif\n",
            encoding="utf-8",
        )
    huge = root / "huge.mk"
    huge.write_text(
        "".join(
            f"VALUE_{i} := $(addprefix src/,a.c b.c c.c)\n"
            f"target{i}:\n\t@echo $(VALUE_{i})\n"
            for i in range(2000 * scale)
        ),
        encoding="utf-8",
    )
    fixes = root / "fixes.mk"
    original = "".join(f"target{i}:\n    echo {i}\n" for i in range(1000 * scale))
    fixes.write_text(original, encoding="utf-8")
    prerequisites = root / "prerequisites"
    prerequisites.mkdir()
    inputs = min(2000, 100 * scale)
    for i in range(inputs):
        (prerequisites / f"source{i}.c").write_text("/* source */\n", encoding="utf-8")
    (prerequisites / "Makefile").write_text(
        "".join(f"missing{i}: absent{i}.txt\nworking{i}: source{i}.o\n" for i in range(inputs)),
        encoding="utf-8",
    )
    return [
        ("include-graph", graph / "Makefile", [], None),
        ("huge-file", huge, [], None),
        ("many-fixes", fixes, ["--fix", "--enable", "MK001"], original),
        ("prerequisite-fanout", prerequisites / "Makefile", ["--enable", "MK216"], None),
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/rumk"))
    parser.add_argument("--baseline", type=Path, help="Compare with another release binary")
    parser.add_argument("--scale", type=int, default=10)
    parser.add_argument("--runs", type=int, default=7)
    parser.add_argument("--warmups", type=int, default=2)
    parser.add_argument("--output", type=Path, help="Write raw timings and metadata as JSON")
    args = parser.parse_args()
    if args.scale < 1 or args.runs < 1 or args.warmups < 0:
        parser.error("scale and runs must be positive; warmups must be nonnegative")
    binaries = [args.binary.resolve()]
    if args.baseline:
        binaries.insert(0, args.baseline.resolve())
    result = {"platform": platform.platform(), "scale": args.scale,
              "warmups": args.warmups, "runs": args.runs, "binaries": {}, "workloads": {}}
    for binary in binaries:
        result["binaries"][str(binary)] = hashlib.sha256(binary.read_bytes()).hexdigest()
    with tempfile.TemporaryDirectory(prefix="rumk-benchmark-") as directory:
        root = Path(directory)
        for name, path, flags, original in generate(root, args.scale):
            samples = {str(binary): [] for binary in binaries}
            expected = None
            # Alternate binary order to reduce systematic cache/thermal bias.
            for iteration in range(args.warmups + args.runs):
                for binary in binaries[::1 if iteration % 2 == 0 else -1]:
                    if original is not None:
                        path.write_text(original, encoding="utf-8")
                    command = [str(binary), "--no-config", "check", str(path),
                               "--fail-on", "never", "--output-format", "json", *flags]
                    start = time.perf_counter()
                    completed = subprocess.run(command, cwd=path.parent, capture_output=True,
                                               check=True)
                    elapsed = time.perf_counter() - start
                    # Compare complete diagnostics and fixed bytes outside the timed region.
                    observed = (completed.stdout, completed.stderr, path.read_bytes())
                    if name == "prerequisite-fanout":
                        diagnostics = json.loads(completed.stdout)
                        count = min(2000, 100 * args.scale)
                        if len(diagnostics) != count or any(d["rule"] != "MK216" or "absent" not in d["message"] for d in diagnostics):
                            raise RuntimeError("Prerequisite workload lost defects or flagged a working source")
                    if expected is None:
                        expected = observed
                    elif observed != expected:
                        raise RuntimeError(f"Output mismatch: {name}, {binary}")
                    if original is not None and observed[2] == original.encode():
                        raise RuntimeError("Fix workload did not apply any fixes")
                    if iteration >= args.warmups:
                        samples[str(binary)].append(elapsed)
            result["workloads"][name] = samples
            for binary, values in samples.items():
                print(f"{name}: {statistics.median(values) * 1000:.1f} ms ({binary})",
                      flush=True)
    if args.output:
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
