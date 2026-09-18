#!/usr/bin/env python3
"""Set `[websearch] engine` (and the keyless fallback) in a jcode config.toml.

Idempotent: rewrites the existing `engine = ...` line inside the `[websearch]`
section, inserts one when the section exists without it, and appends a fresh
section when the file has none. Everything else in the file is left untouched,
so the user's other settings survive.
"""

from __future__ import annotations

import pathlib
import sys

SECTION = "[websearch]"


def set_engine(text: str, engine: str, fallback: str) -> str:
    lines = text.splitlines()
    header = next(
        (index for index, line in enumerate(lines) if line.strip() == SECTION),
        None,
    )

    if header is None:
        if lines and lines[-1].strip():
            lines.append("")
        lines += [SECTION, f'engine = "{engine}"', f"fallback_engines = [{fallback}]"]
        return "\n".join(lines) + "\n"

    end = next(
        (index for index in range(header + 1, len(lines)) if lines[index].startswith("[")),
        len(lines),
    )

    engine_at = None
    fallback_at = None
    for index in range(header + 1, end):
        stripped = lines[index].split("#", 1)[0].strip()
        if stripped.startswith("engine") and "=" in stripped:
            engine_at = index
        elif stripped.startswith("fallback_engines") and "=" in stripped:
            fallback_at = index

    if engine_at is not None:
        lines[engine_at] = f'engine = "{engine}"'
    else:
        lines.insert(header + 1, f'engine = "{engine}"')
        end += 1

    if fallback is not None and fallback_at is None:
        lines.insert(end, f"fallback_engines = [{fallback}]")

    return "\n".join(lines) + "\n"


def get_engine(text: str) -> str:
    inside = False
    for line in text.splitlines():
        if line.strip() == SECTION:
            inside = True
            continue
        if inside and line.startswith("["):
            break
        if inside:
            stripped = line.split("#", 1)[0].strip()
            if stripped.startswith("engine") and "=" in stripped:
                return stripped.split("=", 1)[1].strip().strip('"')
    return ""


def main() -> int:
    argv = sys.argv[1:]
    if argv and argv[0] == "--get":
        if len(argv) < 2:
            print("usage: config-set-engine.py --get <config.toml>", file=sys.stderr)
            return 2
        path = pathlib.Path(argv[1])
        text = path.read_text(encoding="utf-8") if path.exists() else ""
        print(get_engine(text))
        return 0
    if len(argv) < 2:
        print(f"usage: {sys.argv[0]} <config.toml> <engine> [fallback-list]", file=sys.stderr)
        return 2
    path = pathlib.Path(argv[0])
    engine = argv[1]
    fallback = argv[2] if len(argv) > 2 else None
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    path.write_text(set_engine(text, engine, fallback), encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
