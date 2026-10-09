#!/usr/bin/env python3
"""Generate the Tauri update manifest for a signed macOS ARM / Windows x64 release."""
import argparse
import base64
import json
import re
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote


def read_updater_signature(signature_path):
    """Return the base64 Tauri updater signature, validating its container format."""
    if not signature_path.is_file():
        raise ValueError(f"Missing updater signature: {signature_path.name}")
    encoded = signature_path.read_text().strip()
    try:
        decoded = base64.b64decode(encoded, validate=True).decode()
    except (ValueError, UnicodeDecodeError) as exc:
        raise ValueError("Invalid updater signature") from exc
    if not decoded.startswith("untrusted comment:"):
        raise ValueError("Expected a Tauri updater signature")
    return encoded


def require_artifact(artifact_path, name_suffix, description):
    if not artifact_path.is_file() or artifact_path.stat().st_size == 0 or not artifact_path.name.endswith(name_suffix):
        raise ValueError(f"Missing {description}")


def make_manifest(version, repository, archive, signature, notes, windows_setup=None, windows_signature=None):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Release version must be a stable SemVer")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*/[A-Za-z0-9][A-Za-z0-9_.-]*", repository):
        raise ValueError("Repository must have the form owner/name")
    require_artifact(archive, ".app.tar.gz", "macOS updater archive")
    if (windows_setup is None) != (windows_signature is None):
        raise ValueError("Windows setup and signature must be provided together")
    encoded = read_updater_signature(signature)
    platforms = {
        "darwin-aarch64": {
            "signature": encoded,
            "url": f"https://github.com/{repository}/releases/download/v{version}/{quote(archive.name, safe='')}",
        }
    }
    if windows_setup is not None:
        require_artifact(windows_setup, "_x64-setup.exe", "Windows NSIS updater installer")
        platforms["windows-x86_64"] = {
            "signature": read_updater_signature(windows_signature),
            "url": f"https://github.com/{repository}/releases/download/v{version}/{quote(windows_setup.name, safe='')}",
        }
    return {
        "version": version,
        "notes": notes,
        "pub_date": datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
        "platforms": platforms,
    }


def main():
    parser = argparse.ArgumentParser()
    for name in ["repository", "archive", "signature", "notes", "output"]:
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--windows-setup", help="Windows NSIS setup.exe produced by the Windows build")
    parser.add_argument("--windows-signature", help="Minisign signature of the Windows setup.exe")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    config = json.loads((root / "src-tauri/tauri.conf.json").read_text())
    version = config["version"]
    if args.tag != f"v{version}":
        parser.error("Git tag must match the application version")
    endpoint = f"https://github.com/{args.repository}/releases/latest/download/latest.json"
    if endpoint not in config["plugins"]["updater"]["endpoints"]:
        parser.error("Release repository must match the configured update endpoint")
    manifest = make_manifest(
        version,
        args.repository,
        Path(args.archive),
        Path(args.signature),
        Path(args.notes).read_text(),
        windows_setup=Path(args.windows_setup) if args.windows_setup else None,
        windows_signature=Path(args.windows_signature) if args.windows_signature else None,
    )
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")


if __name__ == "__main__":
    main()
