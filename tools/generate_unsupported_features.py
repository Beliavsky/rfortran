"""Generate the unsupported-only view of docs/feature-status.md."""

import argparse
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "docs/feature-status.md"
OUTPUT = ROOT / "docs/unsupported-features.md"


def cells(line):
    return [cell.strip() for cell in re.split(r"(?<!\\)\|", line.strip().strip("|"))]


def generate(source):
    output = [
        "# Unsupported features",
        "",
        "> Generated file—edit [feature-status.md](feature-status.md), not this page.",
        "",
        "Only features marked ❌ (not supported) are shown. Partially supported",
        "features and known wrong results remain in the [full status page](feature-status.md).",
        "Standard versions, notes, and test references are copied from that page.",
        "",
        "Regenerate with `python tools/generate_unsupported_features.py`.",
        "Check freshness with `python tools/generate_unsupported_features.py --check`.",
    ]
    headings = []
    emitted = []
    lines = source.splitlines()
    count = 0
    i = 0
    while i < len(lines):
        line = lines[i]
        heading = re.match(r"^(#{2,6})\s+", line)
        if heading:
            level = len(heading[1])
            headings = [(n, text) for n, text in headings if n < level]
            headings.append((level, line))
        if line.startswith("|") and "Status" in cells(line):
            header = cells(line)
            status = header.index("Status")
            if i + 1 >= len(lines) or not all(
                re.fullmatch(r":?-+:?", cell) for cell in cells(lines[i + 1])
            ):
                raise ValueError("Status table is missing its separator row")
            table = lines[i:i + 2]
            i += 2
            rows = []
            while i < len(lines) and lines[i].startswith("|"):
                row = cells(lines[i])
                if len(row) <= status:
                    raise ValueError(f"Status cell missing: {lines[i]}")
                if row[status] == "❌":
                    rows.append(lines[i])
                i += 1
            if rows:
                # Emit the section hierarchy only when it contains a matching row.
                shared = 0
                while shared < min(len(headings), len(emitted)) and headings[shared] == emitted[shared]:
                    shared += 1
                for _, title in headings[shared:]:
                    output.extend(["", title])
                emitted = headings.copy()
                output.extend(["", *table, *rows])
                count += len(rows)
            continue
        i += 1
    if not count:
        output.extend(["", "No features are currently marked unsupported."])
    # Same-directory relative links remain valid; local anchors refer to the source.
    return "\n".join(output).replace("](#", "](feature-status.md#") + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if the generated page is missing or stale")
    args = parser.parse_args()
    expected = generate(SOURCE.read_text(encoding="utf-8"))
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != expected:
            parser.exit(1, "Unsupported-feature page is stale; run python tools/generate_unsupported_features.py\n")
        print("Unsupported-feature page is current.")
    else:
        OUTPUT.write_text(expected, encoding="utf-8", newline="\n")
        print(f"Wrote {OUTPUT.relative_to(ROOT).as_posix()}")


if __name__ == "__main__":
    main()
