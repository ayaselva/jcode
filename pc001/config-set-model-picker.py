#!/usr/bin/env python3
"""Set jcode's model-picker scope (providers + models) in a config.toml.

Idempotent: rewrites `model_picker_providers` and `model_picker_models` inside
`[provider]` from two line-based list files (blank lines and `#` comments are
ignored). Everything else in the file is left untouched, so the user's other
settings survive.

Usage:
  config-set-model-picker.py <config.toml> <providers-file> <models-file>
  config-set-model-picker.py --get <config.toml> <model_picker_providers|model_picker_models>
"""

from __future__ import annotations

import pathlib
import sys

SECTION = "[provider]"


def read_list(path: str) -> list[str]:
    values: list[str] = []
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines():
        value = line.split("#", 1)[0].strip()
        if value:
            values.append(value)
    return values


def toml_string(value: str) -> str:
    escaped = value.replace("\\", "\\\\").replace('"', '\\"')
    return f'"{escaped}"'


def key_block(key: str, values: list[str]) -> list[str]:
    return [f"{key} = ["] + [f"    {toml_string(value)}," for value in values] + ["]"]


def section_bounds(lines: list[str]) -> tuple[int, int] | None:
    start = next(
        (index for index, line in enumerate(lines) if line.strip() == SECTION),
        None,
    )
    if start is None:
        return None
    end = next(
        (index for index in range(start + 1, len(lines)) if lines[index].startswith("[")),
        len(lines),
    )
    return start, end


def key_span(lines: list[str], start: int, end: int, key: str) -> tuple[int, int] | None:
    """Return the line span [first, last_exclusive) of ``key``'s value."""
    key_at = next(
        (
            index
            for index in range(start + 1, end)
            if lines[index].split("#", 1)[0].strip().startswith(f"{key} =")
            or lines[index].split("#", 1)[0].strip().startswith(f"{key}=")
        ),
        None,
    )
    if key_at is None:
        return None
    code = lines[key_at].split("#", 1)[0]
    if "[" in code and "]" in code:
        return key_at, key_at + 1
    close_at = next(
        (index for index in range(key_at, end) if "]" in lines[index].split("#", 1)[0]),
        None,
    )
    if close_at is None:
        raise ValueError(f"{key} array has no closing bracket")
    return key_at, close_at + 1


def set_key(text: str, key: str, values: list[str]) -> str:
    lines = text.splitlines()
    bounds = section_bounds(lines)
    if bounds is None:
        if lines and lines[-1].strip():
            lines.append("")
        lines += [SECTION] + key_block(key, values)
        return "\n".join(lines) + "\n"

    start, end = bounds
    span = key_span(lines, start, end, key)
    block = key_block(key, values)
    if span is None:
        lines[start + 1 : start + 1] = block
    else:
        first, last = span
        lines[first:last] = block
    return "\n".join(lines) + "\n"


def get_key(text: str, key: str) -> list[str]:
    lines = text.splitlines()
    bounds = section_bounds(lines)
    if bounds is None:
        return []
    start, end = bounds
    span = key_span(lines, start, end, key)
    if span is None:
        return []
    first, last = span
    values: list[str] = []
    for line in lines[first:last]:
        code = line.split("#", 1)[0]
        for raw in code.split('"')[1::2]:
            if raw:
                values.append(raw)
    return values


def main() -> int:
    argv = sys.argv[1:]
    if len(argv) >= 3 and argv[0] == "--get":
        path = pathlib.Path(argv[1])
        text = path.read_text(encoding="utf-8") if path.exists() else ""
        for value in get_key(text, argv[2]):
            print(value)
        return 0

    if len(argv) < 3:
        print(
            f"usage: {sys.argv[0]} <config.toml> <providers-file> <models-file>",
            file=sys.stderr,
        )
        return 2

    config = pathlib.Path(argv[0])
    providers = read_list(argv[1])
    models = read_list(argv[2])
    text = config.read_text(encoding="utf-8") if config.exists() else ""
    text = set_key(text, "model_picker_providers", providers)
    text = set_key(text, "model_picker_models", models)
    config.write_text(text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
