# /// script
# requires-python = ">=3.11"
# ///
"""Embed a changelog for Sparkle's native text view before publishing it."""

import re
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

SPARKLE = "http://www.andymatuschak.org/xml-namespaces/sparkle"
ET.register_namespace("sparkle", SPARKLE)


def commit_notes(tag, notes):
    """GitHub's generated notes omit direct commits, so summarize them too."""
    comparison = re.search(r"https://github\.com/[^\s]+/compare/(\S+)", notes)
    if comparison:
        revision = comparison[1].rstrip("/").replace("...", "..", 1)
    else:
        previous = subprocess.run(
            [
                "git",
                "describe",
                "--tags",
                "--abbrev=0",
                "--match",
                "v[0-9]*",
                f"{tag}^",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        revision = (
            f"{previous.stdout.strip()}..{tag}" if previous.returncode == 0 else tag
        )
    subjects = subprocess.check_output(
        ["git", "log", "--no-merges", "--reverse", "--format=%s", revision], text=True
    ).splitlines()
    bullets = []
    for subject in subjects:
        if re.match(r"chore(?:\(release\))?: (?:release|bump version)\b", subject):
            continue
        summary = re.sub(r"^[a-z]+(?:\([^)]*\))?!?:\s*", "", subject)
        # Commit subjects are text; prevent Markdown syntax from changing them.
        summary = re.sub(r"([\\`*_{}\[\]<>()#+.!|~-])", r"\\\1", summary)
        bullets.append(f"- {summary[:1].upper()}{summary[1:]}")
    if not bullets:
        return notes or "See the full release notes for details."
    return "## What's changed\n\n" + "\n".join(bullets) + "\n\n" + notes


def main():
    notes_path, appcast_path = map(Path, sys.argv[1:])
    appcast = ET.parse(appcast_path)
    item = appcast.find("./channel/item")
    release_link = item.find(f"{{{SPARKLE}}}releaseNotesLink")
    release_url = release_link.text.strip()
    tag = release_url.rsplit("/", 1)[1]

    notes = notes_path.read_text(encoding="utf-8").strip()
    content = re.sub(r"(?im)^\*\*Full Changelog\*\*:.*$", "", notes).strip()
    if not content:
        notes = commit_notes(tag, notes)
    notes_path.write_text(notes + "\n", encoding="utf-8")

    item.remove(release_link)
    ET.SubElement(item, f"{{{SPARKLE}}}fullReleaseNotesLink").text = release_url
    # Markdown selects Sparkle's native NSTextView instead of its HTML web view.
    native_notes = re.sub(
        r"(?im)^\*\*Full Changelog\*\*: (https://\S+)\s*$",
        r"[Full changelog](\1)",
        notes,
    )
    ET.SubElement(
        item, "description", {f"{{{SPARKLE}}}format": "markdown"}
    ).text = native_notes
    appcast.write(appcast_path, encoding="utf-8", xml_declaration=True)


if __name__ == "__main__":
    main()
