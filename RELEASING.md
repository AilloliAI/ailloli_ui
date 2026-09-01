# Releasing Ailloli UI

This document defines the reviewable release procedure. It does not authorize
a commit, push, tag, GitHub Release, crates.io publication, repository setting,
or credential operation. Each remote mutation requires separate maintainer
approval naming the repository, revision, and operation.

## Release lifecycle

- **candidate**: manifests, archives, documentation, and tooling are prepared
  locally; the worktree may still contain reviewed changes.
- **release-ready**: the release commit is clean, pushed, fully validated by CI,
  and taggable without further source changes.
- **tagged**: the annotated version tag points to the exact validated commit.
- **published**: all 22 crates are present and verified on crates.io, followed
  by the matching GitHub pre-release.

These states are one-way. Never move or recreate a published tag, and never
overwrite an immutable crates.io version.

## Synchronized release contract

The 22 `ailloli_ui_*` framework crates share one version and are published
together. During beta releases, every normal, optional, and build dependency
between framework crates carries both a local `path` and the exact registry
requirement for the synchronized version.

The test suite of `ailloli_ui_winit` has one deliberate path-only dev-dependency
on the façade. This breaks a registry publication cycle and is removed by Cargo
from the published manifest. Release tooling verifies that it is the only
exception.

`sandbox_app` and `xtask` always remain `publish = false`. The façade crate
`ailloli_ui` is published last.

## Prepare and inspect a candidate

Start from a reviewed branch using Rust 1.88 and run:

```sh
cargo xtask audit
cargo xtask package-check --allow-dirty
cargo xtask release-plan
cargo xtask release-check --state candidate --allow-dirty
```

The audit validates the closed package set, metadata, exact first-party
requirements, governance, links, workflow pins, assets, secrets, and public
content boundaries. `package-check` inspects `cargo package --list` and the
actual compressed archive for every publishable crate. `release-plan` computes
the publication DAG from Cargo metadata rather than a hand-maintained order.

Before first publication, dependent archives are assembled with Cargo's
`--exclude-lockfile` option. This avoids pretending that unpublished
first-party registry packages can already resolve while preserving Cargo's
real file selection and normalized manifest. The later `cargo publish
--dry-run` remains the authoritative registry-resolution and build check.

Review each archive for required source and assets, normalized registry
dependencies, license metadata, README content, and size. Generated files must
be reproducible or have documented origin and licensing. Never place
credentials, financial data, signing material, private source, or local paths
in release evidence.

## Select the release-ready commit

Before selecting the final commit:

1. Finalize the dated entry in [CHANGELOG.md](CHANGELOG.md).
2. Confirm that GitHub Pages serves the Rustdoc landing page and every crate URL.
3. Run the complete Rust 1.88 CI matrix, CodeQL, RustSec, repository audit,
   `cargo xtask package-check`, and the clean-worktree check:

   ```sh
   cargo xtask release-check --state release-ready
   ```

4. Confirm that the source revision and packaged provenance refer to the public
   `AilloliAI/ailloli_ui` repository, not a private source checkout.
5. Freeze the validated `main` revision. No source modification is permitted
   between green CI and tag creation.

The repository audit always requires `.github/FUNDING.yml` to contain the
canonical `AilloliAI` beneficiary. Sponsorship remains voluntary and is not a
dependency of crates.io publication.

## Tag the validated revision

After dedicated approval, create one annotated tag on the exact green SHA:

```sh
git tag -a v0.1.0-beta.1 -m "Ailloli UI v0.1.0-beta.1"
git push origin v0.1.0-beta.1
cargo xtask release-check --state tagged
```

Do not push any other tag and do not modify the tag after publication.

## First crates.io publication

The first beta is intentionally manual. Keep the crates.io token outside Git,
logs, shell history, and release artifacts. Follow the exact output of:

```sh
cargo xtask release-plan
```

For each crate in one level, run `cargo publish --dry-run` from a clean public
checkout, then publish only after that dry-run succeeds. Wait until every crate
in the current level is resolvable from crates.io before advancing. Publish
`ailloli_ui` last.

The first levels can be dry-run before publication. Higher levels cannot fully
resolve from the registry until their lower-level first-party dependencies are
available; this is an expected registry boundary, not permission to bypass a
failed check.

After each upload, verify version, repository, documentation, Rust version,
license, README, and dependency requirements. Once all crates are available:

```sh
cargo xtask verify-release --version 0.1.0-beta.1
```

Only then create the matching GitHub Release as a pre-release.

## Ownership and later automation

After the initial manual publication, a separately approved operation may add
the `AilloliAI/crates-io-publishers` team as owner of each crate. Trusted
Publishing can then replace long-lived GitHub secrets with temporary OIDC
credentials.

Automated publication is out of scope for the first beta. A later workflow may
publish synchronized releases only from an explicitly approved version tag;
it must never publish from an ordinary push to `main`.

## Rollback and correction

If a candidate is invalid before publication, discard the candidate tag if it
has not been pushed and prepare a corrected commit. If a published version is
invalid, document it as withdrawn or yank it when appropriate, then prepare a
new pre-release version. Never replace already distributed source.
