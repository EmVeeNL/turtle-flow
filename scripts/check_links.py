#!/usr/bin/env python3
"""Check local Markdown links and heading anchors in Markdown files."""

from __future__ import annotations

import re
import sys
import unicodedata
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
LINK = re.compile(r"(?<!!)\[[^\]]*\]\((?:<([^>]+)>|([^\s)]+))(?:\s+[^)]*)?\)")
HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$", re.MULTILINE)


def slug(text: str) -> str:
    text = unicodedata.normalize("NFKD", text).lower()
    text = "".join(c for c in text if not unicodedata.combining(c))
    text = re.sub(r"[^\w\- ]", "", text, flags=re.UNICODE)
    return re.sub(r"\s", "-", text).strip("-")


def anchors(path: Path) -> set[str]:
    content = path.read_text(encoding="utf-8")
    result: set[str] = set()
    counts: dict[str, int] = {}
    for title in HEADING.findall(content):
        base = slug(title)
        n = counts.get(base, 0)
        counts[base] = n + 1
        result.add(base if n == 0 else f"{base}-{n}")
    result.update(re.findall(r'<(?:a|span|div)\s+id=["\']([^"\']+)', content, re.IGNORECASE))
    return result


def main() -> int:
    errors: list[str] = []
    for source in sorted(ROOT.rglob("*.md")):
        if any(part in {"node_modules", ".git", "target"} for part in source.parts):
            continue
        text = source.read_text(encoding="utf-8")
        text = re.sub(r"```.*?```|~~~.*?~~~", "", text, flags=re.DOTALL)
        text = re.sub(r"`[^`]*`", "", text)
        for raw in LINK.findall(text):
            target = unquote(raw[0] or raw[1])
            parts = urlsplit(target)
            if parts.scheme or target.startswith("//"):
                continue
            destination = (source.parent / parts.path).resolve() if parts.path else source.resolve()
            if not destination.exists():
                errors.append(f"{source.relative_to(ROOT)}: missing {target}")
                continue
            if parts.fragment and destination.suffix.lower() == ".md":
                if parts.fragment not in anchors(destination):
                    errors.append(f"{source.relative_to(ROOT)}: missing anchor {target}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Markdown links and anchors are valid.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
