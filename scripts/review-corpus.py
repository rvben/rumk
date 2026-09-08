#!/usr/bin/env python3
"""Apply pinned review labels to a corpus audit without executing builds."""
import argparse
from collections import Counter
import json
from pathlib import Path

CLASSES = {"actionable", "intentional", "false_positive", "partly_false_positive", "needs_context"}


def review(audit, labels):
    if audit.get("schema_version") != 1 or labels.get("schema_version") != 1:
        raise ValueError("Unsupported audit or review schema")
    for field in ("extra_rules", "external_variables"):
        if sorted(audit.get(field, [])) != sorted(labels["profile"][field]):
            raise ValueError(f"Audit profile differs: {field}")
    findings = {}
    roots = {}
    for name, project in audit["projects"].items():
        if project["failures"]:
            raise ValueError(f"Failed audit cannot be scored: {name}")
        for root in project["files"]:
            roots[name, root["file"]] = root
            for diagnostic in json.loads(root["check"]["stdout"]):
                key = (name, root["file"], diagnostic["file"], diagnostic["rule"], diagnostic["line"], diagnostic["column"])
                if key in findings:
                    raise ValueError(f"Ambiguous diagnostic identity: {key}")
                findings[key] = diagnostic
    cases = []
    reviewed = set()
    for case in labels["cases"]:
        name = case["project"]
        project = audit["projects"][name]
        root = roots[name, case["root"]]
        if (project["revision"] != case["revision"]
                or root["input_sha256"] != case["root_sha256"]
                or project["inputs"].get(case["file"]) != case["file_sha256"]):
            raise ValueError(f"Stale review source: {case['id']}")
        if case["classification"] not in CLASSES or not case["reason"].strip():
            raise ValueError(f"Invalid review label: {case['id']}")
        key = (name, case["root"], case["file"], case["rule"], case["line"], case["column"])
        if key in reviewed or any(previous["id"] == case["id"] for previous in cases):
            raise ValueError(f"Duplicate review: {case['id']}")
        reviewed.add(key)
        cases.append({"id": case["id"], "classification": case["classification"],
                      "present": key in findings, "reason": case["reason"]})
    present = [case for case in cases if case["present"]]
    return {"schema_version": 1, "binary_sha256": audit["binary_sha256"],
            "findings_total": len(findings), "reviewed_findings_present": len(present),
            "unreviewed_findings": len(findings.keys() - reviewed),
            "reviewed_findings_absent": len(cases) - len(present),
            "present_classifications": dict(sorted(Counter(case["classification"] for case in present).items())),
            "cases": cases,
            "method": "Pinned, selected review labels; absent findings are not proof of fixes. Unreviewed findings remain unclassified. No population precision, recall, or superiority score is inferred."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--audit", type=Path, required=True)
    parser.add_argument("--labels", type=Path, default=Path(__file__).with_name("corpus-review.json"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = review(json.loads(args.audit.read_text()), json.loads(args.labels.read_text()))
    except (ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
