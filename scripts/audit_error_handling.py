#!/usr/bin/env python3
"""Audit production panicking sites in `src/` Rust code.

For Milestone X / issue #383 — classifies every `panic!` / `unwrap()` /
`expect("…")` call into production vs in-test buckets.  See
`docs/error_handling_audit.md` for the methodology and the canonical
classification of the production sites.

Usage:
    python3 scripts/audit_error_handling.py [path] [--json]

Default `path` is `src/`.  With `--json`, dumps the full production-site
list to `/tmp/audit_prod_sites.json`.

The script intentionally distinguishes std panicking forms from method names
that happen to be called `expect` (e.g. the lexer's
`lex.expect(&Token::Foo)`).
"""

from __future__ import annotations
import json
import os
import re
import sys

# `panic!(` macro call — word-boundary lookbehind avoids matching helpers
# named `…_panic!`.
RE_PANIC = re.compile(r"(?<![A-Za-z_])panic!\s*\(")
# `.unwrap()` — empty parens distinguish std unwrap from `unwrap_or…`.
RE_UNWRAP = re.compile(r"\.unwrap\s*\(\s*\)")
# `.expect("…")` — leading string literal distinguishes std expect from the
# parser's lexer-style `expect(&Token::Foo)` method.
RE_EXPECT = re.compile(r'\.expect\s*\(\s*"')
# Lines that introduce a #[cfg(test)] item — the next `mod NAME {` opens a
# test-only region tracked via brace depth.
RE_CFG_TEST = re.compile(r"#\[cfg\(\s*test\s*\)\]")


def scan(path: str) -> list[tuple[int, str, bool, str]]:
    """Return (line_no, kind, in_test, snippet) for each panicking site."""
    try:
        with open(path, errors="replace") as f:
            lines = f.read().splitlines()
    except OSError:
        return []
    in_test = False
    test_depth = 0
    cfg_test_pending = False
    out: list[tuple[int, str, bool, str]] = []
    for i, line in enumerate(lines, start=1):
        if not in_test and RE_CFG_TEST.search(line):
            cfg_test_pending = True
        if cfg_test_pending and "mod " in line and "{" in line:
            in_test = True
            test_depth = line.count("{") - line.count("}")
            cfg_test_pending = False
            continue
        if in_test:
            test_depth += line.count("{") - line.count("}")
            if test_depth <= 0:
                in_test = False
        snippet = line.strip()[:120]
        if RE_PANIC.search(line):
            out.append((i, "panic", in_test, snippet))
        if RE_UNWRAP.search(line):
            out.append((i, "unwrap", in_test, snippet))
        if RE_EXPECT.search(line):
            out.append((i, "expect", in_test, snippet))
    return out


def main() -> None:
    root = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else "src"
    emit_json = "--json" in sys.argv

    prod_sites: list[dict] = []
    by_crate: dict[str, list] = {}
    test_count = 0

    for dirpath, _, files in os.walk(root):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            rel = os.path.relpath(os.path.join(dirpath, fn))
            parts = rel.split(os.sep)
            if any(p in ("tests", "examples", "benches") for p in parts):
                continue
            crate = "/".join(parts[:2]) if len(parts) >= 2 else parts[0]
            for line_no, kind, in_test, snippet in scan(rel):
                if in_test:
                    test_count += 1
                else:
                    record = {
                        "crate": crate,
                        "file": rel,
                        "line": line_no,
                        "kind": kind,
                        "text": snippet,
                    }
                    prod_sites.append(record)
                    by_crate.setdefault(crate, []).append(record)

    print(f"{'crate':<40} {'prod':>6}")
    print("-" * 50)
    for crate in sorted(by_crate):
        print(f"{crate:<40} {len(by_crate[crate]):>6}")
    print("-" * 50)
    print(f"{'TOTAL production sites':<40} {len(prod_sites):>6}")
    print(f"(test sites excluded: {test_count})")

    if emit_json:
        out_path = "/tmp/audit_prod_sites.json"
        with open(out_path, "w") as fh:
            json.dump(prod_sites, fh, indent=2)
        print(f"wrote {out_path} ({len(prod_sites)} sites)")


if __name__ == "__main__":
    main()
