#!/usr/bin/env python3
"""Acceptance checks for the Python library, kept outside the agent-visible clone."""

from __future__ import annotations

import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check.py <py-01..05> <project-path>", file=sys.stderr)
        return 2
    benchmark, project = sys.argv[1], Path(sys.argv[2]).resolve()
    sys.path.insert(0, str(project / "src"))
    from textkit import slugify

    try:
        if benchmark == "py-01":
            assert slugify("  Hello,   world! ") == "hello-world", "punctuation/spacing normalization failed"
        elif benchmark == "py-02":
            assert slugify("Hello, world!", separator="_") == "hello_world", "custom separator failed"
            try:
                slugify("hello", separator="")
            except ValueError:
                pass
            else:
                raise AssertionError("empty separator should raise ValueError")
        elif benchmark == "py-03":
            assert slugify("hello wonderful world", max_length=12) == "hello-wonder", "maximum length failed"
            try:
                slugify("hello", max_length=0)
            except ValueError:
                pass
            else:
                raise AssertionError("max_length below one should raise ValueError")
        elif benchmark == "py-04":
            assert slugify("Crème Brûlée") == "creme-brulee", "Unicode transliteration failed"
        elif benchmark == "py-05":
            assert slugify("Hello World", lowercase=False) == "Hello-World", "case-preserving slug failed"
        else:
            raise ValueError(f"unknown Python benchmark: {benchmark}")
    except (AssertionError, TypeError, ValueError) as error:
        print(f"{benchmark} failed for ref-py-lib: {error}", file=sys.stderr)
        return 1
    print(f"{benchmark} acceptance passed for ref-py-lib")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
