# Releasing Ailloli UI

This document describes the reviewable beta-release procedure. It does not authorize a tag,
GitHub Release, crates.io publication, or remote mutation.
Every such operation requires a separate maintainer approval naming the target
repository, branch, revision, and artifact.

## Prepare a beta candidate

1. Start from a clean, reviewed `main` revision.
2. Select a SemVer pre-release such as `0.1.0-beta.1` in
   `[workspace.package]`; every package must inherit it.
3. Keep every package `publish = false` until crates.io publication is approved
   as a separate project decision.
4. Update [CHANGELOG.md](CHANGELOG.md) with user-visible changes, migrations,
   known limitations, and the candidate status.
5. Verify package metadata, first-party links, licenses, NOTICE files,
   third-party provenance, and the public asset manifest.
6. Run the complete Rust 1.88 CI matrix, Rustdoc with warnings denied,
   repository audits, secret scans, RustSec audit, and deterministic sandbox
   capture.
7. Reproduce the public tree through the documented deterministic projection
   process and prove tree equality before any remote action.
8. Review the exact commits, generated documentation, capture, audit reports,
   and known skips with a human maintainer.

## Provenance and artifacts

Release evidence must identify the source commit, public tree, Rust toolchain,
lockfile, commands, platform, and SHA-256 of generated artifacts. Generated
files must be reproducible or have a documented origin and license. Never copy
credentials, signing material, financial data, private source, or local paths
into release evidence.

## Publish only after dedicated approval

After all local and remote-candidate gates pass, stop and request approval for
each of the following independently:

- creating and pushing a signed or annotated Git tag;
- creating a GitHub Release and uploading artifacts;
- changing any package from `publish = false` and publishing to crates.io;
- enabling or changing repository security, Pages, or sponsorship settings.

Verify the remote revision and public artifacts immediately after every
approved operation. A beta candidate is not a published release until those
operations have happened and been verified.

## Rollback and correction

Do not move or silently replace a published tag. If a candidate is invalid,
document it as withdrawn and prepare a new pre-release version. Security fixes
follow [SECURITY.md](SECURITY.md); avoid public detail until coordinated
disclosure is appropriate.
