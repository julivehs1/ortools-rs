#!/usr/bin/env python3
"""Rewrite the PREBUILT_SHA256 table in ortools-src/build.rs.

Reads the `digests.txt` published alongside a release — `<target-triple>
<sha256>` per line — and replaces the table wholesale, so a target that
vanished from a release also vanishes here instead of lingering with a stale
digest.

Deliberately not driven by SHA256SUMS: build.rs turns a triple's hyphens into
underscores to form the tarball name, and `x86_64` already contains an
underscore, so a file name cannot be parsed back into the triple it came from.
CI records the mapping instead, where the build matrix still knows it.

Usage: update-digests.py <digests.txt>
"""

import pathlib
import re
import sys

BUILD_RS = pathlib.Path(__file__).resolve().parent.parent / "crates/ortools-src/build.rs"
TRIPLE = re.compile(r"[A-Za-z0-9_]+(-[A-Za-z0-9_.]+)+")
DIGEST = re.compile(r"[0-9a-f]{64}")


def parse(path):
    entries = {}
    for lineno, line in enumerate(pathlib.Path(path).read_text().splitlines(), 1):
        line = line.split("#")[0].strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2:
            sys.exit(f"{path}:{lineno}: expected `<target> <sha256>`, got {line!r}")
        target, digest = parts
        if not TRIPLE.fullmatch(target):
            sys.exit(f"{path}:{lineno}: {target!r} is not a target triple")
        if not DIGEST.fullmatch(digest):
            sys.exit(f"{path}:{lineno}: {digest!r} is not a sha256 digest")
        if target in entries and entries[target] != digest:
            sys.exit(f"{path}:{lineno}: conflicting digests for {target}")
        entries[target] = digest
    return dict(sorted(entries.items()))


def render(entries):
    body = "".join(f'    ("{t}", "{d}"),\n' for t, d in entries.items())
    return f"const PREBUILT_SHA256: &[(&str, &str)] = &[\n{body}];\n"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)

    entries = parse(sys.argv[1])
    if not entries:
        sys.exit(f"no digests in {sys.argv[1]}")

    source = BUILD_RS.read_text()
    pattern = re.compile(
        r"^const PREBUILT_SHA256: &\[\(&str, &str\)\] = &\[.*?^\];\n",
        re.M | re.S,
    )
    if not pattern.search(source):
        sys.exit(f"could not find the PREBUILT_SHA256 table in {BUILD_RS}")

    BUILD_RS.write_text(pattern.sub(lambda _: render(entries), source, count=1))
    for target, digest in entries.items():
        print(f"  {target:<28} {digest[:16]}\u2026")
    print(f"wrote {len(entries)} digests to {BUILD_RS}")


if __name__ == "__main__":
    main()
