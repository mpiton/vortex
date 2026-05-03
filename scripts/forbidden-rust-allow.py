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

Exits 1 with the hit list on stdout when any are found, 0 otherwise. Used
by the `forbidden-tools` CI job and is safe to run locally.
"""
import re
import subprocess
import sys

FORBIDDEN = re.compile(
    r"\b(dead_code|unused|unused_variables|unused_imports)\b"
)
ALLOW_GROUP = re.compile(r"#\[allow\(([^)]*)\)\]", re.DOTALL)
MAX_HITS = 20


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
        for m in ALLOW_GROUP.finditer(text):
            if FORBIDDEN.search(m.group(1)):
                line = text[: m.start()].count("\n") + 1
                snippet = " ".join(m.group(0).split())[:120]
                hits.append(f"{f}:{line}: {snippet}")
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
