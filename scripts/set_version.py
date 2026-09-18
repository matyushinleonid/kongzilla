#!/usr/bin/env python3
"""Stamp one version across every file that carries one.

The crates, the web package, the Helm chart and the deployed image tag all name
a version, and a release that sets them by hand eventually sets one of them
wrong. This takes the version once and writes it everywhere, failing loudly if
a file it expected to change did not.

Usage: scripts/set_version.py 0.2.0
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
ROOT = Path(__file__).resolve().parent.parent


def substitute(path: Path, pattern: str, replacement: str) -> None:
    """Applies one substitution, insisting that it matched exactly once."""
    source = path.read_text()
    updated, count = re.subn(pattern, replacement, source, count=1, flags=re.MULTILINE)
    if count != 1:
        raise SystemExit(f"{path}: {pattern!r} matched {count} times, expected 1")
    path.write_text(updated)


def set_json_version(path: Path, version: str) -> None:
    """Rewrites a package version without reformatting the rest of the file."""
    if not path.exists():
        return
    document = json.loads(path.read_text())
    if document.get("version") is None:
        raise SystemExit(f"{path}: no version field")
    substitute(path, r'^(\s*"version":\s*)"[^"]*"', r'\1"%s"' % version)
    # A lockfile names the package twice: once at the top and once for itself.
    if path.name == "package-lock.json" and '"": {' in path.read_text():
        substitute(path, r'^(    "": \{\n      "name": "[^"]*",\n      "version": )"[^"]*"', r'\1"%s"' % version)


def main(argv: list[str]) -> int:
    if len(argv) != 2 or not SEMVER.match(argv[1]):
        print(__doc__, file=sys.stderr)
        return 2
    version = argv[1]

    substitute(
        ROOT / "Cargo.toml",
        r'^(\[workspace\.package\]\n(?:.*\n)*?version = )"[^"]*"',
        r'\1"%s"' % version,
    )
    set_json_version(ROOT / "web" / "package.json", version)
    set_json_version(ROOT / "web" / "package-lock.json", version)

    chart = ROOT / "deploy" / "helm" / "kongzilla" / "Chart.yaml"
    substitute(chart, r"^version: .*$", f"version: {version}")
    substitute(chart, r"^appVersion: .*$", f'appVersion: "{version}"')

    substitute(
        ROOT / "deploy" / "helm" / "kongzilla" / "values.yaml",
        r"^(image:\n(?:  .*\n)*?  tag:).*$",
        r'\1 "%s"' % version,
    )

    print(f"stamped {version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
