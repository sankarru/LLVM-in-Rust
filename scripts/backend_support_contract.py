#!/usr/bin/env python3
"""Validate the backend support contract fixture."""

from __future__ import annotations

import argparse
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MATRIX = ROOT / "docs" / "backend_support_matrix.json"
DOC = ROOT / "docs" / "backend_support_matrix.md"

REQUIRED_AXES = [
    "abi",
    "calls",
    "varargs",
    "structs_by_value_aggregates",
    "atomics",
    "fp_simd",
    "eh_unwind",
    "debug_info",
    "object_format",
    "relocations",
    "linker_execution",
]
REQUIRED_TARGETS = ["x86_64", "aarch64", "riscv64", "wasm32"]
VALID_STATUSES = {"supported", "partial", "experimental", "unsupported"}
REQUIRED_E2E = {
    "c_frontend",
    "rustc_backend_smoke",
    "lto",
    "debug_unwind",
    "sanitizer_instrumented",
}


def rel_exists(path: str) -> bool:
    return (ROOT / path).exists()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SystemExit(message)


def validate_evidence(owner: str, evidence: object, required: bool) -> int:
    if evidence is None:
        evidence = []
    require(isinstance(evidence, list), f"{owner}: evidence must be a list")
    if required:
        require(evidence, f"{owner}: evidence is required")

    count = 0
    for idx, item in enumerate(evidence):
        where = f"{owner}.evidence[{idx}]"
        require(isinstance(item, dict), f"{where}: evidence item must be an object")
        path = item.get("path")
        command = item.get("command")
        require(isinstance(path, str) and path.strip(), f"{where}: path is required")
        require(rel_exists(path), f"{where}: referenced path does not exist: {path}")
        require(
            isinstance(command, str) and command.strip(),
            f"{where}: command is required",
        )
        count += 1
    return count


def validate_matrix() -> None:
    require(MATRIX.exists(), f"missing {MATRIX.relative_to(ROOT)}")
    require(DOC.exists(), f"missing {DOC.relative_to(ROOT)}")

    data = json.loads(MATRIX.read_text())
    require(data.get("schema_version") == 1, "schema_version must be 1")
    require(data.get("policy") == "docs/backend_support_matrix.md", "policy path mismatch")
    require(data.get("issue") == 384, "issue must be 384")
    require(data.get("axes") == REQUIRED_AXES, "axes changed without updating validator")

    targets = data.get("targets")
    require(isinstance(targets, dict), "targets must be an object")
    require(sorted(targets) == sorted(REQUIRED_TARGETS), "target set mismatch")

    status_counts = {status: 0 for status in VALID_STATUSES}
    evidence_count = 0
    for target in REQUIRED_TARGETS:
        target_obj = targets[target]
        require(isinstance(target_obj.get("display"), str), f"{target}: display is required")
        cells = target_obj.get("cells")
        require(isinstance(cells, dict), f"{target}: cells must be an object")
        require(sorted(cells) == sorted(REQUIRED_AXES), f"{target}: cell axes mismatch")

        for axis in REQUIRED_AXES:
            owner = f"{target}.{axis}"
            cell = cells[axis]
            require(isinstance(cell, dict), f"{owner}: cell must be an object")
            status = cell.get("status")
            require(status in VALID_STATUSES, f"{owner}: invalid status {status!r}")
            status_counts[status] += 1

            needs_evidence = status in {"supported", "partial"}
            evidence_count += validate_evidence(owner, cell.get("evidence"), needs_evidence)

            if status == "partial":
                require(
                    str(cell.get("limitations", "")).strip(),
                    f"{owner}: partial cells need limitations",
                )
            if status in {"experimental", "unsupported"}:
                marker = cell.get("marker")
                prefix = "not_supported:" if status == "unsupported" else "experimental:"
                require(
                    isinstance(marker, str) and marker.startswith(prefix),
                    f"{owner}: {status} cells need marker prefix {prefix}",
                )
                require(
                    str(cell.get("limitations", "")).strip(),
                    f"{owner}: {status} cells need limitations",
                )

    e2e = data.get("release_blocking_e2e")
    require(isinstance(e2e, dict), "release_blocking_e2e must be an object")
    require(set(e2e) == REQUIRED_E2E, "release_blocking_e2e key mismatch")
    for key in sorted(REQUIRED_E2E):
        item = e2e[key]
        owner = f"release_blocking_e2e.{key}"
        require(isinstance(item, dict), f"{owner}: entry must be an object")
        workflow = item.get("workflow")
        require(
            isinstance(workflow, str) and rel_exists(workflow),
            f"{owner}: workflow path missing: {workflow}",
        )
        require(str(item.get("scope", "")).strip(), f"{owner}: scope is required")
        evidence_count += validate_evidence(owner, item.get("evidence"), required=True)

    print(
        "validated backend support matrix: "
        f"{len(REQUIRED_TARGETS)} targets, {len(REQUIRED_AXES)} axes, "
        f"{evidence_count} evidence item(s), statuses={status_counts}"
    )


def main() -> None:
    global MATRIX
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--matrix",
        default=str(MATRIX),
        help="matrix path; defaults to docs/backend_support_matrix.json",
    )
    args = parser.parse_args()

    MATRIX = (ROOT / args.matrix).resolve() if not Path(args.matrix).is_absolute() else Path(args.matrix)
    validate_matrix()


if __name__ == "__main__":
    main()
