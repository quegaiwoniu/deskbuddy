#!/usr/bin/env python3
"""Generate the Tauri update manifest for a signed macOS ARM release."""
import argparse
import base64
import json
import re
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote


def make_manifest(version, repository, archive, signature, notes):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Release version must be a stable SemVer")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*", repository):
        raise ValueError("Repository must have the form owner/name")
    if not archive.is_file() or archive.stat().st_size == 0 or not archive.name.endswith(".app.tar.gz"):
        raise ValueError("Missing macOS updater archive")
    encoded = signature.read_text().strip()
    try:
        decoded = base64.b64decode(encoded, validate=True).decode()
    except (ValueError, UnicodeDecodeError) as exc:
        raise ValueError("Invalid updater signature") from exc
    if not decoded.startswith("untrusted comment:"):
        raise ValueError("Expected a Tauri updater signature")
    return {
        "version": version,
        "notes": notes,
        "pub_date": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "platforms": {
            "darwin-aarch64": {
                "signature": encoded,
                "url": f"https://github.com/{repository}/releases/download/v{version}/{quote(archive.name, safe='')}",
            }
        },
    }


def main():
    parser = argparse.ArgumentParser()
    for name in ["repository", "archive", "signature", "notes", "output"]:
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
    version = config["version"]
    if args.tag != f"v{version}":
        parser.error("Git tag must match the application version")
    endpoint = f"https://github.com/{args.repository}/releases/latest/download/latest.json"
    if endpoint not in config["plugins"]["updater"]["endpoints"]:
        parser.error("Release repository must match the configured update endpoint")
    manifest = make_manifest(version, args.repository, Path(args.archive), Path(args.signature), Path(args.notes).read_text())
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
