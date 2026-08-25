#!/usr/bin/env python3
"""Fail-closed Cargo metadata audit for the public Ailloli UI workspace."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any


VERSION = "0.1.0-beta.1"
AUTHORS = ["Rising Corporation and Ailloli UI contributors"]
LICENSE = "Apache-2.0 OR MIT"
MSRV = "1.88"
REPOSITORY = "https://github.com/AilloliAI/ailloli_ui"
HOMEPAGE = "https://ailloliai.github.io/ailloli_ui/"

EXPECTED_CATEGORIES = {
    "ailloli_ui": ["gui"],
    "ailloli_ui_app_storage": ["filesystem"],
    "ailloli_ui_bench": ["development-tools"],
    "ailloli_ui_core": ["gui"],
    "ailloli_ui_devicons_font": ["graphics", "rendering"],
    "ailloli_ui_devtools_core": ["development-tools"],
    "ailloli_ui_devtools_ui": ["development-tools"],
    "ailloli_ui_editor": ["text-processing"],
    "ailloli_ui_fs": ["filesystem"],
    "ailloli_ui_fs_local": ["filesystem"],
    "ailloli_ui_fs_runtime": ["filesystem"],
    "ailloli_ui_icon": ["graphics", "rendering"],
    "ailloli_ui_openxr": ["graphics", "rendering"],
    "ailloli_ui_packaging": ["development-tools"],
    "ailloli_ui_render_vulkan": ["graphics", "rendering"],
    "ailloli_ui_render_wgpu": ["graphics", "rendering"],
    "ailloli_ui_runtime": ["gui"],
    "ailloli_ui_terminal_core": ["command-line-utilities"],
    "ailloli_ui_terminal_pty": ["command-line-utilities"],
    "ailloli_ui_text": ["text-processing"],
    "ailloli_ui_widgets": ["gui"],
    "ailloli_ui_winit": ["gui"],
    "sandbox_app": ["development-tools"],
}

KEYWORD = re.compile(r"^[a-z0-9][a-z0-9-]{0,19}$")


class AuditFailure(RuntimeError):
    """A closed metadata contract was not met."""


def fail(message: str) -> None:
    raise AuditFailure(message)


def validate_package_fields(package: dict[str, Any], expected_name: str) -> None:
    expected = {
        "name": expected_name,
        "version": VERSION,
        "authors": AUTHORS,
        "license": LICENSE,
        "rust_version": MSRV,
        "repository": REPOSITORY,
        "homepage": HOMEPAGE,
        "documentation": f"{HOMEPAGE}{expected_name}/",
        "publish": [],
        "categories": EXPECTED_CATEGORIES[expected_name],
    }
    for key, value in expected.items():
        if package.get(key) != value:
            fail(
                f"package {expected_name!r} has unexpected {key}: "
                f"{package.get(key)!r}; expected {value!r}"
            )

    description = package.get("description")
    if not isinstance(description, str) or len(description.strip()) < 20:
        fail(f"package {expected_name!r} needs a specific non-empty description")

    keywords = package.get("keywords")
    if not isinstance(keywords, list) or not 1 <= len(keywords) <= 5:
        fail(f"package {expected_name!r} needs one to five keywords")
    if len(keywords) != len(set(keywords)) or any(
        not isinstance(keyword, str) or not KEYWORD.fullmatch(keyword)
        for keyword in keywords
    ):
        fail(f"package {expected_name!r} has invalid or duplicate keywords")


def cargo_metadata(root: Path) -> dict[str, Any]:
    command = [
        os.environ.get("CARGO", "cargo"),
        "metadata",
        "--locked",
        "--format-version",
        "1",
        "--manifest-path",
        str(root / "Cargo.toml"),
    ]
    process = subprocess.run(
        command,
        cwd=root,
        env={
            **os.environ,
            "CARGO_NET_OFFLINE": os.environ.get("CARGO_NET_OFFLINE", "true"),
            "CARGO_TERM_COLOR": "never",
        },
        check=False,
        capture_output=True,
        text=True,
    )
    if process.returncode != 0:
        detail = process.stderr.strip() or process.stdout.strip()
        fail(f"cargo metadata --locked failed: {detail}")
    try:
        value = json.loads(process.stdout)
    except json.JSONDecodeError as error:
        fail(f"cargo metadata returned invalid JSON: {error}")
    if not isinstance(value, dict):
        fail("cargo metadata root must be an object")
    return value


def validate_workspace(root: Path) -> dict[str, Any]:
    metadata = cargo_metadata(root)
    if Path(metadata.get("workspace_root", "")).resolve() != root.resolve():
        fail("Cargo workspace root does not match the audited repository")

    packages = metadata.get("packages")
    members = metadata.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(members, list):
        fail("cargo metadata omits packages or workspace_members")
    by_id = {
        package.get("id"): package
        for package in packages
        if isinstance(package, dict) and isinstance(package.get("id"), str)
    }
    try:
        workspace_packages = [by_id[member] for member in members]
    except KeyError as error:
        fail(f"workspace member is absent from package metadata: {error}")

    by_name = {package.get("name"): package for package in workspace_packages}
    if len(workspace_packages) != 23 or set(by_name) != set(EXPECTED_CATEGORIES):
        fail(
            "workspace package set must be exactly 22 framework crates plus "
            "sandbox_app"
        )

    descriptions: set[str] = set()
    for name in sorted(EXPECTED_CATEGORIES):
        package = by_name[name]
        validate_package_fields(package, name)
        description = package["description"]
        if description in descriptions:
            fail(f"package {name!r} reuses another package description")
        descriptions.add(description)
        manifest = Path(package.get("manifest_path", "")).resolve()
        try:
            manifest.relative_to(root.resolve())
        except ValueError:
            fail(f"package {name!r} manifest escapes the public workspace")

    sandbox_dependencies = {
        dependency.get("name") for dependency in by_name["sandbox_app"]["dependencies"]
    }
    if sandbox_dependencies != {"ailloli_ui"}:
        fail(
            "sandbox_app must depend directly only on ailloli_ui; got "
            f"{sorted(str(item) for item in sandbox_dependencies)}"
        )

    versions: dict[str, set[str]] = {}
    for package in packages:
        if isinstance(package, dict) and isinstance(package.get("name"), str):
            versions.setdefault(package["name"], set()).add(str(package.get("version")))
    if versions.get("lru") != {"0.18.2"}:
        fail(f"lru must resolve exactly to 0.18.2; got {versions.get('lru')}")
    if versions.get("winit") != {"0.30.13"}:
        fail(f"winit must remain exactly 0.30.13; got {versions.get('winit')}")
    if not versions.get("wgpu") or any(
        not version.startswith("0.20.") for version in versions["wgpu"]
    ):
        fail(f"wgpu must remain on the 0.20 line; got {versions.get('wgpu')}")

    return {
        "status": "ok",
        "packages": len(workspace_packages),
        "version": VERSION,
        "publish_false": len(workspace_packages),
        "lru": "0.18.2",
    }


def run_self_test(script_dir: Path) -> dict[str, Any]:
    fixtures = script_dir / "fixtures"
    valid = json.loads((fixtures / "metadata-valid.json").read_text(encoding="utf-8"))
    invalid = json.loads(
        (fixtures / "metadata-invalid.json").read_text(encoding="utf-8")
    )
    validate_package_fields(valid, "ailloli_ui")
    try:
        validate_package_fields(invalid, "ailloli_ui")
    except AuditFailure:
        pass
    else:
        fail("negative metadata fixture was unexpectedly accepted")
    return {"status": "ok", "positive": 1, "negative": 1}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="public workspace root",
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        result = (
            run_self_test(Path(__file__).resolve().parent)
            if args.self_test
            else validate_workspace(args.root.resolve())
        )
    except (AuditFailure, OSError, json.JSONDecodeError) as error:
        if args.json:
            print(json.dumps({"status": "error", "error": str(error)}, sort_keys=True))
        else:
            print(f"metadata-audit: ERROR: {error}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(result, sort_keys=True))
    else:
        print(f"metadata-audit: PASS: {result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
