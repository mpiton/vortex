#!/usr/bin/env python3
"""Multi-line aware scanner for forbidden `#[allow(...)]` suppressions.

`git grep -E` is line-oriented, so a multiline form like

    #[allow(
        clippy::module_name_repetitions,
        dead_code,
    )]

slips through a single-line regex. This script slurps each `*.rs` file once
and walks every `#[allow(...)]` group with `re.DOTALL`, then reports a
`file:line: snippet` hit when one of the forbidden tokens appears anywhere
inside the captured argument list.

Behaviour:
- Both outer (`#[allow(...)]`) and inner (`#![allow(...)]`) attribute forms
  are inspected.
- Line and block comments are stripped before scanning so a comment that
  literally mentions `#[allow(dead_code)]` (e.g. a TODO note) does not
  produce a false positive.
- A suppression is tolerated when the immediately preceding non-blank line
  is a `// TODO(task-N): ...` comment that documents the cleanup task.
  This is the only escape hatch — silent suppressions still fail the gate.

Exits 1 with the hit list on stdout when any are found, 0 otherwise. Any
other exit code (e.g. propagated exception) signals a real failure that
the CI step must abort on.
"""
from __future__ import annotations

import re
import subprocess
import sys

FORBIDDEN = re.compile(
    r"\b(dead_code|unused|unused_variables|unused_imports)\b"
)
# Outer `#[allow(...)]` and inner `#![allow(...)]`. Optional whitespace
# around the brackets / `allow` keyword. Lazy `(.*?)` body so the match
# terminates at the first `)]` even when the body itself contains
# parenthesised forms (e.g. `clippy::needless_pass_by_value`).
ALLOW_GROUP = re.compile(r"#!?\[\s*allow\s*\((.*?)\)\s*\]", re.DOTALL)
TODO_TASK = re.compile(r"^\s*//\s*TODO\(\s*[A-Za-z0-9_-]+\s*\)\s*:")
MAX_HITS = 20


def strip_comments_preserving_lines(text: str) -> str:
    """Remove `//` and `/* ... */` comments while keeping line numbers stable.

    Walks the source as a small state machine so a `/*` inside a string
    literal isn't treated as a comment opener, and a `"` inside a comment
    isn't treated as a string opener. Stripped spans are replaced with the
    same number of newlines so `text[:pos].count("\\n")` still maps to the
    original line numbers.

    Handles the common Rust forms — line comments, block comments
    (`/* ... */`), string literals (`"..."` with `\\` escapes), and char
    literals (`'.'`). Raw strings (`r"..."`, `r#"..."#`) and multi-line
    char literals are not modelled exhaustively; they degrade to no-strip
    for the affected span, which is the safe direction (false positives
    are reported and reviewed; false negatives would silently let a
    forbidden suppression through).
    """
    out: list[str] = []
    n = len(text)
    i = 0
    while i < n:
        c = text[i]
        # `"..."` string literal
        if c == '"':
            out.append(c)
            i += 1
            while i < n:
                if text[i] == "\\" and i + 1 < n:
                    out.append(text[i])
                    out.append(text[i + 1])
                    i += 2
                    continue
                out.append(text[i])
                if text[i] == '"':
                    i += 1
                    break
                i += 1
            continue
        # `'x'` char literal (best-effort: just skip until the closing `'`
        # without crossing a newline, so a stray `'` in a comment-mention
        # doesn't open a fake literal).
        if c == "'":
            j = i + 1
            buf = [c]
            while j < n and text[j] not in ("\n", "'"):
                if text[j] == "\\" and j + 1 < n:
                    buf.append(text[j])
                    buf.append(text[j + 1])
                    j += 2
                    continue
                buf.append(text[j])
                j += 1
            if j < n and text[j] == "'":
                buf.append(text[j])
                out.extend(buf)
                i = j + 1
                continue
            # Not a real char literal — emit as plain char and move on.
            out.append(c)
            i += 1
            continue
        # `// ...` line comment
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            nl = text.find("\n", i)
            if nl == -1:
                # rest of file is a comment; keep no newline (none follows)
                break
            i = nl
            continue
        # `/* ... */` block comment — Rust permits nesting, so track depth.
        # `/* outer /* inner */ still outer */` is one comment.
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            depth = 1
            j = i + 2
            close = -1
            while j < n - 1:
                if text[j] == "/" and text[j + 1] == "*":
                    depth += 1
                    j += 2
                    continue
                if text[j] == "*" and text[j + 1] == "/":
                    depth -= 1
                    if depth == 0:
                        close = j
                        break
                    j += 2
                    continue
                j += 1
            if close == -1:
                # Unterminated — skip the rest, preserving newlines so
                # downstream line numbers still map.
                rest = text[i:]
                out.append("\n" * rest.count("\n"))
                i = n
                continue
            inside = text[i : close + 2]
            out.append("\n" * inside.count("\n"))
            i = close + 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


def line_offset(text: str, line_num: int) -> int:
    """Return byte offset of the start of `line_num` (1-indexed) in `text`."""
    if line_num <= 1:
        return 0
    pos = 0
    for _ in range(line_num - 1):
        nl = text.find("\n", pos)
        if nl == -1:
            return len(text)
        pos = nl + 1
    return pos


def previous_nonblank_line(text: str, pos: int) -> str:
    """Return the previous non-blank line ending before `pos` (or '')."""
    head = text[:pos]
    lines = head.split("\n")
    # The last entry in `lines` is the partial line containing `pos`;
    # walk backwards looking for a non-blank predecessor.
    for line in reversed(lines[:-1]):
        if line.strip():
            return line
    return ""


def main() -> int:
    files = subprocess.check_output(
        ["git", "ls-files", "*.rs"], text=True
    ).split()
    hits: list[str] = []
    for f in files:
        try:
            with open(f, encoding="utf-8", errors="replace") as fh:
                text = fh.read()
        except OSError:
            continue
        stripped = strip_comments_preserving_lines(text)
        for m in ALLOW_GROUP.finditer(stripped):
            if not FORBIDDEN.search(m.group(1)):
                continue
            line_num = stripped[: m.start()].count("\n") + 1
            # Documented-TODO escape hatch: the line above must be a
            # `// TODO(task-N): ...` comment. The match offset is in
            # `stripped`; map it back to the original text via line_num
            # so the comment we removed is still observable.
            orig_offset = line_offset(text, line_num)
            prior = previous_nonblank_line(text, orig_offset)
            if TODO_TASK.match(prior):
                continue
            snippet = " ".join(m.group(0).split())[:120]
            hits.append(f"{f}:{line_num}: {snippet}")
            if len(hits) >= MAX_HITS:
                break
        if len(hits) >= MAX_HITS:
            break
    if hits:
        print("\n".join(hits))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
