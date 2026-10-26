#!/usr/bin/env python3
"""Acceptance checks for the Rust CLI, kept outside the agent-visible clone."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check.py <rust-01..05> <project-path>", file=sys.stderr)
        return 2
    benchmark, project = sys.argv[1], Path(sys.argv[2]).resolve()
    cases = {
        "rust-02": (["--unique"], "oak pine oak\n", "2"),
        "rust-03": (["--min-length", "3"], "a cat bird fish\n", "3"),
        "rust-04": (["--lines"], "one two\n  \ntre\n", "2"),
        "rust-05": (["--ignore-case"], "Oak oak PINE Pine\n", "2"),
    }
    command = ["cargo", "run", "--quiet", "--locked", "--manifest-path", str(project / "Cargo.toml"), "--"]
    try:
        if benchmark == "rust-01":
            result = subprocess.run(
                [*command, "--json"], input="one two\n", text=True, capture_output=True, check=True
            )
            if json.loads(result.stdout) != {"words": 2}:
                raise AssertionError("JSON word count did not match")
        elif benchmark in cases:
            args, stdin, expected = cases[benchmark]
            result = subprocess.run([*command, *args], input=stdin, text=True, capture_output=True, check=True)
            if result.stdout.strip() != expected:
                raise AssertionError(f"expected {expected!r}, got {result.stdout.strip()!r}")
            if benchmark == "rust-03":
                invalid = subprocess.run(
                    [*command, "--min-length"], input=stdin, text=True, capture_output=True
                )
                if invalid.returncode == 0:
                    raise AssertionError("missing --min-length value should fail")
        else:
            raise ValueError(f"unknown Rust benchmark: {benchmark}")
    except (AssertionError, json.JSONDecodeError, subprocess.CalledProcessError, ValueError) as error:
        print(f"{benchmark} failed for ref-rust-cli: {error}", file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr.rstrip(), file=sys.stderr)
        return 1
    print(f"{benchmark} acceptance passed for ref-rust-cli")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
