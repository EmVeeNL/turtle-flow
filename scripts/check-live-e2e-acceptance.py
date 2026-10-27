#!/usr/bin/env python3
"""Dispatch an acceptance check stored outside the agent-visible fixture clone."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BENCHMARKS = {
    **{f"rust-{n:02d}": "ref-rust-cli" for n in range(1, 6)},
    **{f"ts-{n:02d}": "ref-ts-api" for n in range(1, 6)},
    **{f"py-{n:02d}": "ref-py-lib" for n in range(1, 6)},
}


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in BENCHMARKS:
        print("usage: check-live-e2e-acceptance.py <rust-01..05|ts-01..05|py-01..05> <project-path>", file=sys.stderr)
        return 2
    benchmark, project = sys.argv[1], Path(sys.argv[2]).resolve()
    fixture = BENCHMARKS[benchmark]
    if not project.is_dir():
        print(f"project directory does not exist: {project}", file=sys.stderr)
        return 2
    if benchmark.startswith("ts-"):
        command = ["node", "--experimental-strip-types", str(ROOT / "fixtures/acceptance/ref-ts-api/check.mjs"), str(project), benchmark]
    else:
        command = [
            sys.executable,
            str(ROOT / f"fixtures/acceptance/{fixture}/check.py"),
            benchmark,
            str(project),
        ]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
