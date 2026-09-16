#!/usr/bin/env python3
"""Add a jcode provider profile (and its model-picker entry) to a config.toml.

Idempotent: the block is appended only when the ``[providers.<name>]`` header of
the block file is absent, and the picker entry is inserted into
``model_picker_providers`` inside ``[provider]`` only when it is missing.
Everything else in the file is left untouched, so the user's other settings
survive.

Usage:
  config-add-provider.py <config.toml> <provider-block.toml> <picker-entry>
  config-add-provider.py --get <config.toml> <provider-name>
  config-add-provider.py --get-picker <config.toml> <picker-entry>
"""

from __future__ import annotations

import pathlib
import sys


def block_header(block: str) -> str:
    for line in block.splitlines():
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            return stripped
    raise ValueError("provider block has no table header")


def has_section(lines: list[str], header: str) -> bool:
    return any(line.strip() == header for line in lines)


def section_bounds(lines: list[str], header: str) -> tuple[int, int] | None:
    start = next(
        (index for index, line in enumerate(lines) if line.strip() == header),
        None,
    )
    if start is None:
        return None
    end = next(
        (index for index in range(start + 1, len(lines)) if lines[index].startswith("[")),
        len(lines),
    )
    return start, end


def picker_entry_present(lines: list[str], entry: str) -> bool:
    return any(entry in line for line in lines)


def split_inline_array(value: str) -> list[str]:
    """Split the contents of a one-line TOML array into its raw elements."""
    inner = value[value.index("[") + 1 : value.rindex("]")]
    items: list[str] = []
    current = ""
    in_string = False
    for char in inner:
        if char == '"':
            in_string = not in_string
        if char == "," and not in_string:
            if current.strip():
                items.append(current.strip())
            current = ""
            continue
        current += char
    if current.strip():
        items.append(current.strip())
    return items


def add_picker_entry(text: str, entry: str) -> str:
    """Insert ``entry`` into ``[provider] model_picker_providers``."""
    lines = text.splitlines()
    bounds = section_bounds(lines, "[provider]")
    if bounds is None:
        if lines and lines[-1].strip():
            lines.append("")
        lines += ["[provider]", f'model_picker_providers = ["{entry}"]']
        return "\n".join(lines) + "\n"

    start, end = bounds
    key_at = next(
        (
            index
            for index in range(start + 1, end)
            if lines[index].split("#", 1)[0].strip().startswith("model_picker_providers")
        ),
        None,
    )
    if key_at is None:
        lines.insert(end, f'model_picker_providers = ["{entry}"]')
        return "\n".join(lines) + "\n"

    if picker_entry_present(lines[key_at:end], entry):
        return text

    # One-line array: rebuild the element list in place.
    if "[" in lines[key_at] and "]" in lines[key_at]:
        line = lines[key_at]
        head = line[: line.index("[") + 1]
        tail = line[line.rindex("]") :]
        items = split_inline_array(line)
        items.append(f'"{entry}"')
        lines[key_at] = f"{head}{', '.join(items)}{tail}"
        return "\n".join(lines) + "\n"

    close_at = next(
        (
            index
            for index in range(key_at + 1, end)
            if lines[index].rstrip().endswith("]")
        ),
        None,
    )
    if close_at is None:
        raise ValueError("model_picker_providers array has no closing bracket")

    elements = [
        index
        for index in range(key_at + 1, close_at)
        if lines[index].strip() and not lines[index].strip().startswith("#")
    ]
    indent = (
        lines[elements[-1]][: len(lines[elements[-1]]) - len(lines[elements[-1]].lstrip())]
        if elements
        else lines[key_at][: len(lines[key_at]) - len(lines[key_at].lstrip())] + "    "
    )
    if elements and not lines[elements[-1]].rstrip().endswith(","):
        lines[elements[-1]] = lines[elements[-1]].rstrip() + ","
    lines.insert(close_at, f'{indent}"{entry}",')
    return "\n".join(lines) + "\n"


def add_provider_block(text: str, block: str) -> str:
    lines = text.splitlines()
    header = block_header(block)
    if has_section(lines, header):
        return text
    body = block.rstrip("\n")
    if lines and lines[-1].strip():
        lines.append("")
    lines += body.splitlines()
    return "\n".join(lines) + "\n"


def get_provider_value(text: str, name: str) -> str:
    lines = text.splitlines()
    bounds = section_bounds(lines, f"[providers.{name}]")
    if bounds is None:
        return ""
    start, end = bounds
    for index in range(start + 1, end):
        stripped = lines[index].split("#", 1)[0].strip()
        if stripped.startswith("default_model") and "=" in stripped:
            return stripped.split("=", 1)[1].strip().strip('"')
    return ""


def main() -> int:
    argv = sys.argv[1:]
    if not argv:
        print(f"usage: {sys.argv[0]} <config.toml> <provider-block.toml> <picker-entry>", file=sys.stderr)
        return 2

    if argv[0] == "--get":
        if len(argv) < 3:
            print(f"usage: {sys.argv[0]} --get <config.toml> <provider-name>", file=sys.stderr)
            return 2
        path = pathlib.Path(argv[1])
        text = path.read_text(encoding="utf-8") if path.exists() else ""
        print(get_provider_value(text, argv[2]))
        return 0

    if argv[0] == "--get-picker":
        if len(argv) < 3:
            print(f"usage: {sys.argv[0]} --get-picker <config.toml> <picker-entry>", file=sys.stderr)
            return 2
        path = pathlib.Path(argv[1])
        text = path.read_text(encoding="utf-8") if path.exists() else ""
        entry = argv[2]
        print(entry if picker_entry_present(text.splitlines(), entry) else "")
        return 0

    if len(argv) < 3:
        print(f"usage: {sys.argv[0]} <config.toml> <provider-block.toml> <picker-entry>", file=sys.stderr)
        return 2

    config = pathlib.Path(argv[0])
    block = pathlib.Path(argv[1]).read_text(encoding="utf-8")
    entry = argv[2]
    text = config.read_text(encoding="utf-8") if config.exists() else ""
    updated = add_picker_entry(add_provider_block(text, block), entry)
    config.write_text(updated, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
