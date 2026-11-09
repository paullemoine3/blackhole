"""Release notes of a version, from the in-app changelog (crates/domain/src/changelog.rs).

Usage: python3 scripts/release_notes.py 0.8.0 > notes.md
Prints the notes as Markdown with the files of the release; exits 1 when the version has no
entry in the changelog (a release must not go out without its notes).
"""
import re
import sys
from pathlib import Path

CHANGELOG = Path(__file__).resolve().parent.parent / "crates" / "domain" / "src" / "changelog.rs"
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')


def unescape(text: str) -> str:
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), text)


def notes(version: str) -> list[str]:
    source = CHANGELOG.read_text(encoding="utf-8")
    start = source.find(f'"{version}",')
    if start < 0:
        return []
    body = source[start + len(version) + 3:]
    end = body.find("],")
    return [unescape(m.group(1)) for m in LITERAL.finditer(body[: end if end >= 0 else len(body)])]


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    version = sys.argv[1].lstrip("v")
    items = notes(version)
    if not items:
        print(f"no changelog entry for {version} in {CHANGELOG}", file=sys.stderr)
        return 1
    out = [f"## BlackHole {version}", ""]
    out += [f"- {item}" for item in items]
    out += [
        "",
        "### Downloads",
        "",
        f"- Windows: `BlackHole-Setup-{version}-x64.exe` (installer) or `BlackHole-windows-x64.zip` (portable)",
        "- macOS (Apple silicon and Intel): `BlackHole-macos-universal.dmg`",
        "- Android 10+: `BlackHole-android.apk` (or `BlackHole-android-debug.apk` when the release key is not configured)",
        "",
        "Check the files against `SHA256SUMS.txt`. The apps already installed on Windows and macOS update themselves.",
    ]
    sys.stdout.reconfigure(encoding="utf-8")
    print("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main())
