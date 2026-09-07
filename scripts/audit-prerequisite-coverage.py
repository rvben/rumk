#!/usr/bin/env python3
"""Measure MK216 decisions on pinned, clean checkouts without executing Make."""
import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
import subprocess

SPEC = importlib.util.spec_from_file_location("audit", Path(__file__).with_name("audit-corpus.py"))
AUDIT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(AUDIT)


def summarize(roots):
    blockers, occurrences, outcomes = Counter(), Counter(), Counter()
    for root in roots:
        coverage = root["coverage"]
        blockers.update(coverage["root_blockers"].keys())
        occurrences.update(coverage["root_blockers"])
        outcomes.update(coverage["outcomes"])
        assert sum(coverage["outcomes"].values()) == len(coverage["edges"])
    return {"roots": len(roots), "eligible_roots": sum(not r["coverage"]["root_blockers"] for r in roots),
            "roots_by_blocker": dict(blockers.most_common()),
            "blocker_occurrences": dict(occurrences.most_common()),
            "edge_outcomes": dict(outcomes.most_common()), "visible_edges": sum(outcomes.values())}


def audit(binary, root, timeout):
    if AUDIT.git(root, "status", "--porcelain", "--untracked-files=no").strip():
        raise RuntimeError(f"Tracked files must be clean: {root}")
    before = AUDIT.inventory(root)
    paths = AUDIT.makefiles(root)
    if not paths:
        raise RuntimeError(f"No tracked Makefiles: {root}")
    command = [str(binary), *paths]
    first = subprocess.run(command, cwd=root, capture_output=True, check=True, timeout=timeout)
    second = subprocess.run(command, cwd=root, capture_output=True, check=True, timeout=timeout)
    if first.stdout != second.stdout or first.stderr or second.stderr:
        raise RuntimeError(f"Coverage output was not repeatable: {root}")
    if AUDIT.inventory(root) != before:
        raise RuntimeError(f"Files changed during coverage audit: {root}")
    roots = json.loads(first.stdout)
    if [r["root"] for r in roots] != paths:
        raise RuntimeError(f"Coverage root inventory mismatch: {root}")
    return {"revision": AUDIT.git(root, "rev-parse", "HEAD").decode().strip(),
            "inputs": before, "roots": roots, "summary": summarize(roots)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/debug/examples/prerequisite-coverage"))
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--project", type=Path, action="append", required=True)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text())
    roots = [p.resolve() for p in args.project]
    if args.timeout <= 0 or len({p.name for p in roots}) != len(roots) or set(manifest) != {p.name for p in roots}:
        parser.error("positive timeout and exactly one checkout per manifest project required")
    for root in roots:
        if AUDIT.git(root, "rev-parse", "HEAD").decode().strip() != manifest[root.name]["revision"]:
            parser.error(f"{root.name}: revision differs from manifest")
    binary = args.binary.resolve()
    projects = {root.name: audit(binary, root, args.timeout) for root in roots}
    report = {"schema_version": 1, "binary_sha256": AUDIT.digest(binary.read_bytes()),
              "manifest": manifest, "projects": projects,
              "method": "All tracked Makefiles are independent roots, including fragments. Root blockers overlap. Edges are the evaluated semantic inventory, not all runtime dependencies. Possible producers and I/O uncertainty are not proven buildability. Two identical runs; unchanged inventories; no Make execution.",
              "summary": summarize([r for p in projects.values() for r in p["roots"]])}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["summary"], indent=2))


if __name__ == "__main__":
    main()
