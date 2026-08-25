#!/usr/bin/env python3
"""Audit public governance, links, workflows, funding, paths, and secrets."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tomllib
from typing import Any


EXPECTED_FUNDING = "github: AilloliAI\n"
EXPECTED_CODEOWNERS = "* @MrRise-RiCorp\n"
PUBLIC_REPOSITORY = "https://github.com/AilloliAI/ailloli_ui"
PUBLIC_PAGES = "https://ailloliai.github.io/ailloli_ui/"
PUBLIC_SPONSORS = "https://github.com/sponsors/AilloliAI"
ORGANIZATION_REPOSITORY_PREFIX = "https://github.com/" + "AilloliAI/"
ORGANIZATION_PAGES_PREFIX = "https://" + "ailloliai.github.io/"
CAPTURE_PATH = "artifacts/captures/public_sandbox_showcase.png"
CAPTURE_SHA256 = "88920411aafcb8cbc6e9a9e71a5041a627b677cec62da820fd4f8d9be1ba1136"
ICON_PATH = "apps/sandbox_app/src/assets/icons/icon.svg"
ICON_V3_SHA256 = "e8056e11a3e16a21da5e12726c283cea4d43bab2b479a9c8b31401cd2118de43"

REQUIRED_FILES = {
    ".cargo/audit.toml",
    "Cargo.toml",
    "Cargo.lock",
    "ARCHITECTURE.md",
    "BENCHMARKING.md",
    "README.md",
    "SECURITY.md",
    "CONTRIBUTING.md",
    "MIGRATION.md",
    "SUPPORT.md",
    "RELEASING.md",
    "RUSTSEC.md",
    "CHANGELOG.md",
    "SPONSORS.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "NOTICE",
    "docs/index.html",
    ".github/CODEOWNERS",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/pull_request_template.md",
    ".github/dependabot.yml",
    ".github/workflows/ci.yml",
    ".github/workflows/codeql.yml",
    ".github/workflows/pages.yml",
    "artifacts/captures/MANIFEST.toml",
    ".github/scripts/audit-metadata.py",
    ".github/scripts/audit-repository.py",
    ".github/scripts/run-actionlint.sh",
}

EXPECTED_WORKFLOWS = {"ci.yml", "codeql.yml", "pages.yml"}
ALLOWED_ACTIONS = {
    "actions/checkout": "d23441a48e516b6c34aea4fa41551a30e30af803",
    "github/codeql-action/init": "db488ddef3bf6cb639b32c2e9a7c0a7ea8271d28",
    "github/codeql-action/analyze": "db488ddef3bf6cb639b32c2e9a7c0a7ea8271d28",
    "actions/configure-pages": "983d7736d9b0ae728b81ab479565c72886d7745b",
    "actions/upload-pages-artifact": "7b1f4a764d45c48632c6b24a0339c27f5614fb0b",
    "actions/deploy-pages": "d6db90164ac5ed86f2b6aed7e0febac5b3c0c03e",
}

USES = re.compile(r"^\s*-?\s*uses:\s*([^@\s]+)@([^\s#]+)", re.MULTILINE)
URL = re.compile(r"https://[^\s)\]>'\"]+")
MARKDOWN_LINK = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
SECRET_PATTERNS = (
    re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
    re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b"),
    re.compile(r"\bgh[pousr]_[A-Za-z0-9]{36,}\b"),
    re.compile(r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b"),
)
ABSOLUTE_PATHS = (
    re.compile(r"/(?:home|Users)/[A-Za-z0-9._-]+/"),
    re.compile(r"\b[A-Za-z]:\\Users\\[^\\\s]+\\"),
)
CONTEXT_MILESTONE_PATTERNS = (
    re.compile(
        r"(?i)(?<![a-z0-9])(?:pre|post)?[-_. ]*phase[-_. ]*"
        r"\d+(?:[-_.]\d+)*(?![a-z0-9])"
    ),
    re.compile(r"(?i)(?<![a-z0-9])ui[-_. ]*xr[-_. ]*\d+(?![a-z0-9])"),
)
EXCLUDED_DIRECTORIES = {".git", ".cache", "generated", "target", "vendor"}


class AuditFailure(RuntimeError):
    """A repository contract was not met."""


def fail(message: str) -> None:
    raise AuditFailure(message)


def read_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        fail(f"cannot read UTF-8 file {path}: {error}")


def validate_decontextualized_value(value: str, label: str) -> None:
    for pattern in CONTEXT_MILESTONE_PATTERNS:
        match = pattern.search(value)
        if match is not None:
            fail(f"internal development milestone found in {label}: {match.group(0)!r}")


def validate_funding_text(text: str, label: str) -> None:
    if text != EXPECTED_FUNDING:
        fail(f"{label} must contain only the canonical AilloliAI GitHub beneficiary")


def validate_workflow_text(text: str, label: str) -> None:
    for forbidden in (
        "pull_request_target:",
        "self-hosted",
        "permissions: write-all",
        "secrets.",
    ):
        if forbidden in text:
            fail(f"workflow {label} contains forbidden token {forbidden!r}")
    if not re.search(r"(?m)^permissions:\s*\n\s{2}contents:\s*read\s*$", text):
        fail(f"workflow {label} needs top-level permissions: contents: read")

    references = list(USES.finditer(text))
    if not references:
        fail(f"workflow {label} contains no auditable action reference")
    for reference in references:
        action, revision = reference.groups()
        expected = ALLOWED_ACTIONS.get(action)
        if expected is None:
            fail(f"workflow {label} uses unapproved action {action!r}")
        if not re.fullmatch(r"[0-9a-f]{40}", revision):
            fail(f"workflow {label} action {action!r} is not pinned by full SHA")
        if revision != expected:
            fail(f"workflow {label} action {action!r} uses an unreviewed SHA")


def validate_workflows(root: Path, extra_workflow_roots: list[Path]) -> int:
    public_workflows = root / ".github/workflows"
    names = {path.name for path in public_workflows.glob("*.yml") if path.is_file()}
    if names != EXPECTED_WORKFLOWS:
        fail(
            "public workflow set differs from ci.yml, codeql.yml, and pages.yml: "
            f"{sorted(names)}"
        )

    workflow_paths = sorted(public_workflows.glob("*.yml"))
    for extra in extra_workflow_roots:
        resolved = extra.resolve()
        if not resolved.is_dir():
            fail(f"extra workflow root is missing: {resolved}")
        workflow_paths.extend(sorted(resolved.glob("*.yml")))

    for path in workflow_paths:
        text = read_text(path)
        validate_workflow_text(text, str(path))
        name = path.name
        if name == "ci.yml" and re.search(r"(?m)^\s+[a-z-]+:\s*write\s*$", text):
            fail(f"CI workflow {path} must not request write permissions")
        if name == "codeql.yml":
            if text.count("security-events: write") != 1:
                fail("CodeQL must grant security-events: write exactly once")
            if "language: [rust, actions]" not in text or "build-mode: none" not in text:
                fail("CodeQL must analyze rust and actions in build mode none")
        if name == "pages.yml":
            if text.count("pages: write") != 1 or text.count("id-token: write") != 1:
                fail("Pages must grant pages and id-token write exactly once")
            if "needs: build" not in text:
                fail("Pages deployment must be separated from its build job")
    return len(workflow_paths)


def iter_candidate_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for directory, directories, names in os.walk(root, followlinks=False):
        directories[:] = sorted(
            name for name in directories if name not in EXCLUDED_DIRECTORIES
        )
        directory_path = Path(directory)
        for name in sorted(names):
            path = directory_path / name
            if path.is_symlink() or not path.is_file():
                continue
            files.append(path)
    return files


def iter_text_files(files: list[Path]) -> list[Path]:
    text_files: list[Path] = []
    for path in files:
        if path.stat().st_size > 4_000_000:
            continue
        raw = path.read_bytes()
        if b"\0" in raw:
            continue
        try:
            raw.decode("utf-8")
        except UnicodeDecodeError:
            continue
        text_files.append(path)
    return text_files


def validate_candidate_text(root: Path) -> int:
    private_tokens = (
        "ailloli" + "_ui_internal",
        "ailloli" + "_suite",
        "ailloli" + "-ui-internal",
        "AilloliAI/" + "ailloli-ui",
    )
    candidate_files = iter_candidate_files(root)
    for path in candidate_files:
        relative = path.relative_to(root).as_posix()
        validate_decontextualized_value(relative, f"public path {relative}")

    text_files = iter_text_files(candidate_files)
    for path in text_files:
        relative = path.relative_to(root).as_posix()
        text = read_text(path)
        validate_decontextualized_value(text, f"public file {relative}")
        for token in private_tokens:
            if token in text:
                fail(f"private or non-canonical repository token found in {relative}")
        for pattern in SECRET_PATTERNS:
            if pattern.search(text):
                fail(f"high-confidence credential pattern found in {relative}")
        for pattern in ABSOLUTE_PATHS:
            if pattern.search(text):
                fail(f"machine-specific absolute path found in {relative}")
        for url in URL.findall(text):
            cleaned = url.rstrip(".,;:")
            if cleaned.startswith(ORGANIZATION_REPOSITORY_PREFIX) and not cleaned.startswith(
                PUBLIC_REPOSITORY
            ):
                fail(f"non-canonical AilloliAI repository URL in {relative}: {cleaned}")
            if cleaned.startswith(ORGANIZATION_PAGES_PREFIX) and not cleaned.startswith(
                PUBLIC_PAGES
            ):
                fail(f"non-canonical AilloliAI Pages URL in {relative}: {cleaned}")
    return len(text_files)


def validate_commit_subjects(
    root: Path, revision_range: str | None, explicit_subjects: list[str]
) -> int:
    subjects = list(explicit_subjects)
    if revision_range is not None:
        try:
            top_level = subprocess.run(
                ["git", "-C", str(root), "rev-parse", "--show-toplevel"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
            git_root = Path(top_level).resolve()
            public_prefix = root.resolve().relative_to(git_root).as_posix()
            command = [
                "git",
                "-C",
                str(git_root),
                "log",
                "--format=%s",
                revision_range,
            ]
            if public_prefix != ".":
                command.extend(["--", public_prefix])
            output = subprocess.run(
                command,
                check=True,
                capture_output=True,
                text=True,
            ).stdout
        except (OSError, subprocess.CalledProcessError, ValueError) as error:
            fail(f"cannot inspect commit subjects: {error}")
        subjects.extend(line for line in output.splitlines() if line)

    for subject in subjects:
        validate_decontextualized_value(subject, f"commit subject {subject!r}")
    return len(subjects)


def validate_relative_markdown_links(root: Path) -> int:
    count = 0
    for name in (
        "README.md",
        "ARCHITECTURE.md",
        "BENCHMARKING.md",
        "MIGRATION.md",
        "SECURITY.md",
        "CONTRIBUTING.md",
        "SUPPORT.md",
        "RELEASING.md",
        "CHANGELOG.md",
        "SPONSORS.md",
    ):
        path = root / name
        for target in MARKDOWN_LINK.findall(read_text(path)):
            if target.startswith(("https://", "http://", "mailto:", "#")):
                continue
            raw_path = target.split("#", 1)[0]
            if not raw_path:
                continue
            candidate = (path.parent / raw_path).resolve()
            try:
                candidate.relative_to(root.resolve())
            except ValueError:
                fail(f"relative link escapes the repository in {name}: {target}")
            if not candidate.exists():
                fail(f"broken relative link in {name}: {target}")
            count += 1
    return count


def validate_reviewed_assets(root: Path) -> dict[str, Any]:
    manifest_path = root / "artifacts/captures/MANIFEST.toml"
    try:
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        fail(f"invalid capture manifest: {error}")
    if set(manifest) != {"version", "capture"} or manifest["version"] != 1:
        fail("capture manifest must use the closed version 1 schema")
    captures = manifest["capture"]
    if not isinstance(captures, list) or len(captures) != 1:
        fail("capture manifest must declare exactly the public beta final capture")
    capture = captures[0]
    expected = {
        "path": CAPTURE_PATH,
        "sha256": CAPTURE_SHA256,
        "width": 1280,
        "height": 756,
        "license": "Apache-2.0 OR MIT",
    }
    if not isinstance(capture, dict) or set(capture) != {*expected, "provenance"}:
        fail("capture manifest entry has an unexpected schema")
    for key, value in expected.items():
        if capture.get(key) != value:
            fail(f"capture manifest has unexpected {key}: {capture.get(key)!r}")
    provenance = capture.get("provenance")
    if not isinstance(provenance, str) or not all(
        phrase in provenance for phrase in ("public Ailloli UI façade", "settle", "timeout")
    ):
        fail("capture provenance must record façade, settle, and timeout")

    png = root / CAPTURE_PATH
    raw = png.read_bytes()
    if hashlib.sha256(raw).hexdigest() != CAPTURE_SHA256:
        fail("public beta sandbox capture SHA-256 does not match its manifest")
    if len(raw) < 24 or raw[:8] != b"\x89PNG\r\n\x1a\n":
        fail("public beta sandbox capture is not an encoded PNG")
    width, height = struct.unpack(">II", raw[16:24])
    if (width, height) != (1280, 756):
        fail(f"public beta sandbox capture dimensions changed: {width}x{height}")

    icon = root / ICON_PATH
    if hashlib.sha256(icon.read_bytes()).hexdigest() != ICON_V3_SHA256:
        fail("sandbox icon.svg is not the reviewed v3 asset")
    return {"capture": f"{width}x{height}", "icon": "v3"}


def validate_governance(root: Path, require_funding: bool) -> str:
    for relative in sorted(REQUIRED_FILES):
        path = root / relative
        if not path.is_file() or path.stat().st_size == 0:
            fail(f"missing or empty required public file: {relative}")

    if read_text(root / ".github/CODEOWNERS") != EXPECTED_CODEOWNERS:
        fail("CODEOWNERS must assign the complete tree to @MrRise-RiCorp")

    required_phrases = {
        "ARCHITECTURE.md": [
            "Workspace packages",
            "Targeted work and retained trees",
            "invalidating one component never rebuilds",
        ],
        "BENCHMARKING.md": [
            "ailloli-ui-bench",
            "AILLOLI_UI_BENCH_PATH",
            "GPU",
            "device-pixel ratio (DPR)",
        ],
        "MIGRATION.md": [
            "Cargo feature migration",
            "`native-overlay`",
            "`native_overlay`",
        ],
        "SECURITY.md": [
            "private GitHub Security Advisory",
            "Do not open a public issue",
            "best-effort",
            "Sponsorship never buys",
        ],
        "CONTRIBUTING.md": ["Rust 1.88", "Apache License 2.0", "SECURITY.md"],
        "SUPPORT.md": ["best-effort", "Future commercial services", "no guaranteed"],
        "RELEASING.md": ["does not authorize a tag", "crates.io", "separate maintainer approval"],
        "CHANGELOG.md": ["Unreleased", "0.1.0-beta.1", "Unpublished candidate"],
        "SPONSORS.md": [
            "Sponsorship funds the development of Ailloli UI; it does not purchase the",
            "Supporter — 5 USD/month",
            "Backer — 25 USD/month",
            "Bronze Sponsor — 100 USD/month",
            "Silver Sponsor — 250 USD/month",
            "Gold Sponsor — 500 USD/month",
            "Corporate Sponsor — 1,000 USD/month",
            "up to ten monthly tiers",
            "price cannot be",
        ],
    }
    for name, phrases in required_phrases.items():
        text = read_text(root / name)
        for phrase in phrases:
            if phrase not in text:
                fail(f"{name} is missing required policy text {phrase!r}")

    triaged_ids = {
        "RUSTSEC-2024-0436",
        "RUSTSEC-2026-0186",
        "RUSTSEC-2026-0192",
        "RUSTSEC-2026-0206",
    }
    audit_config = tomllib.loads(read_text(root / ".cargo/audit.toml"))
    advisories = audit_config.get("advisories")
    output = audit_config.get("output")
    if not isinstance(advisories, dict) or set(advisories.get("ignore", [])) != triaged_ids:
        fail("cargo-audit ignore list must match the four reviewed advisory IDs")
    if advisories.get("informational_warnings") != ["unmaintained", "unsound"]:
        fail("cargo-audit must continue reporting unmaintained and unsound notices")
    if not isinstance(output, dict) or output.get("deny") != ["warnings"]:
        fail("cargo-audit must fail closed on every new warning")
    triage = read_text(root / "RUSTSEC.md")
    for advisory_id in triaged_ids | {"RUSTSEC-2026-0253"}:
        if advisory_id not in triage:
            fail(f"RUSTSEC.md is missing advisory triage for {advisory_id}")
    if "RUSTSEC-2026-0253" in advisories.get("ignore", []):
        fail("the fixed lru advisory must never be ignored")

    funding = root / ".github/FUNDING.yml"
    if funding.exists():
        validate_funding_text(read_text(funding), ".github/FUNDING.yml")
        funding_status = "verified-file"
    elif require_funding:
        fail(".github/FUNDING.yml is required after Sponsors activation")
    else:
        funding_status = "deferred"

    issue_config = read_text(root / ".github/ISSUE_TEMPLATE/config.yml")
    if f"{PUBLIC_REPOSITORY}/security/advisories/new" not in issue_config:
        fail("issue configuration must redirect vulnerabilities to private advisories")
    if PUBLIC_SPONSORS not in read_text(root / "SPONSORS.md"):
        fail("SPONSORS.md must use the canonical organization Sponsors URL")
    if PUBLIC_PAGES not in read_text(root / "README.md"):
        fail("README.md must link the canonical Pages homepage")
    return funding_status


def run_self_test(script_dir: Path) -> dict[str, int]:
    fixtures = script_dir / "fixtures"
    validate_funding_text(
        read_text(fixtures / "funding-valid.yml"), "positive funding fixture"
    )
    try:
        validate_funding_text(
            read_text(fixtures / "funding-invalid.yml"), "negative funding fixture"
        )
    except AuditFailure:
        pass
    else:
        fail("negative funding fixture was unexpectedly accepted")

    validate_workflow_text(
        read_text(fixtures / "workflow-valid.yml"), "positive workflow fixture"
    )
    try:
        validate_workflow_text(
            read_text(fixtures / "workflow-invalid.yml"), "negative workflow fixture"
        )
    except AuditFailure:
        pass
    else:
        fail("negative workflow fixture was unexpectedly accepted")

    validate_decontextualized_value("semantic public regression", "positive context fixture")
    invalid_values = (
        "legacy-" + "phase" + str(999),
        "legacy-" + "ui" + "-xr" + str(999),
    )
    for value in invalid_values:
        try:
            validate_decontextualized_value(value, "negative context fixture")
        except AuditFailure:
            pass
        else:
            fail("negative context fixture was unexpectedly accepted")
    return {"positive": 3, "negative": 4}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="public repository root",
    )
    parser.add_argument(
        "--extra-workflow-root",
        action="append",
        type=Path,
        default=[],
        help="additional workflow directory, used by the private source repository",
    )
    parser.add_argument(
        "--allow-missing-funding",
        action="store_true",
        help="temporary local preparation mode before Sponsors activation",
    )
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--commit-range", help="new commit range whose subjects must pass")
    parser.add_argument(
        "--commit-subject",
        action="append",
        default=[],
        help="proposed new commit subject to validate",
    )
    parser.add_argument(
        "--commit-subjects-only",
        action="store_true",
        help="validate only the supplied new commit range or subjects",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.self_test:
            result: dict[str, Any] = run_self_test(Path(__file__).resolve().parent)
        else:
            root = args.root.resolve()
            if not root.is_dir():
                fail(f"public repository root is missing: {root}")
            commit_subjects = validate_commit_subjects(
                root, args.commit_range, args.commit_subject
            )
            if args.commit_subjects_only:
                if args.commit_range is None and not args.commit_subject:
                    fail("commit-subjects-only requires a range or explicit subject")
                result = {"commit_subjects": commit_subjects}
            else:
                funding = validate_governance(root, not args.allow_missing_funding)
                workflows = validate_workflows(root, args.extra_workflow_root)
                scanned = validate_candidate_text(root)
                links = validate_relative_markdown_links(root)
                assets = validate_reviewed_assets(root)
                result = {
                    "status": "ok",
                    "funding": funding,
                    "workflows": workflows,
                    "scanned_text_files": scanned,
                    "relative_links": links,
                    "reviewed_assets": assets,
                    "commit_subjects": commit_subjects,
                }
    except (
        AuditFailure,
        OSError,
        UnicodeDecodeError,
        json.JSONDecodeError,
        tomllib.TOMLDecodeError,
    ) as error:
        if args.json:
            print(json.dumps({"status": "error", "error": str(error)}, sort_keys=True))
        else:
            print(f"repository-audit: ERROR: {error}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(result, sort_keys=True))
    else:
        print(f"repository-audit: PASS: {result}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
