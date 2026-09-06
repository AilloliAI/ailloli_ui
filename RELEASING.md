# Releasing Ailloli UI

This document defines the reviewable procedure for any synchronized beta
release. It does not authorize a commit, push, tag, GitHub Release, crates.io
publication, repository setting, or credential operation. Each remote mutation
requires separate maintainer approval naming the repository, revision, version,
and operation.

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

The exact release version comes from `[workspace.package].version`. Release
commands and checks must derive it from Cargo metadata instead of maintaining a
second executable version constant.

The 22 `ailloli_ui_*` framework crates share that version and are published
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
cargo +1.88.0-x86_64-unknown-linux-gnu xtask audit
cargo +1.88.0-x86_64-unknown-linux-gnu xtask package-check --allow-dirty
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-plan
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-check --state candidate --allow-dirty
```

The audit validates the closed package set, metadata, exact first-party
requirements, governance, links, workflow pins, assets, secrets, and public
content boundaries. `package-check` inspects `cargo package --list` and the
lock-excluded source archive for every publishable crate. This full preflight
validates file selection and normalized manifests before unpublished
first-party dependencies exist. Its SHA-256 values are explicitly not evidence
of bytes uploaded to crates.io. `release-plan` computes the publication DAG
from Cargo metadata rather than a hand-maintained order.

With `--allow-dirty`, the manifest records the repository's uncommitted state
separately from each archive's packaged-input state. An unchanged crate may be
clean even when another crate or document is modified. Both the repository and
every archive must still be clean for registry verification of a publication.

Review each archive for required source and assets, normalized registry
dependencies, license metadata, README content, and size. Generated files must
be reproducible or have documented origin and licensing. Never place
credentials, financial data, signing material, private source, or local paths
in release evidence.

## Freeze the changelog and release notes

During development, record notable changes under `[Unreleased]`. At the release
freeze:

1. Audit the complete delta from the previous immutable tag, crate by crate.
   Review source changes and public API differences as well as commit subjects.
   Account for every user-visible feature, improvement, fix, compatibility or
   security change in a concise changelog entry. Keep maintenance under `Project`
   and exclude changes that are absent from the release commit. The maintainer
   must review this coverage: a text comparison cannot discover a feature omitted
   from the changelog itself.
2. Rename the populated section to the exact workspace version and the freeze
   date.
3. Create a new empty `[Unreleased]` section immediately above it.
4. Point `[Unreleased]` from the new tag to `HEAD` and add the canonical release
   reference for the frozen version.
5. Confirm that the previous release section and reference remain unchanged.

Generate the proposed GitHub Release body from the changelog rather than
maintaining a second editorial source:

```sh
mkdir -p target/xtask-package-check
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-notes \
  > target/xtask-package-check/release-notes.md
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-notes \
  --check target/xtask-package-check/release-notes.md
```

The command emits only the dated current workspace version section and refuses
to substitute `[Unreleased]`. Candidate validation may inspect `[Unreleased]`
before the freeze, but release-note extraction cannot. Review the output before
it is passed to any remote release operation. The `--check` gate compares every
section, entry, and continuation line with the complete dated changelog body.
Only CRLF/LF line endings and terminal newlines may differ. Empty notes, a short
summary, a changelog link, omitted improvements, extra features, or another
version's notes fail the gate. A successful check prints the file's SHA-256.

Review both completeness of the changelog against the release delta and the
rendered Markdown body before approving the tag. Preserve the exact checked file
as release evidence. The `workflow_dispatch` run of `release.yml` on `main` must
pass these checks before a new tag is created; the tag run repeats them. Both
`release-ready` and `tagged` require `--notes-file`, even when archive generation
is skipped. The workflow uploads that same file with the source manifest.

Until the matching GitHub Release exists and returns successfully, active
candidate links must point to the public `CHANGELOG.md` on `main`. Only the
declarative changelog reference for the frozen version may point to the future
canonical release URL.

## Select the release-ready commit

Before selecting the final commit:

1. Confirm that GitHub Pages serves the Rustdoc landing page and every crate URL.
2. Run the complete Rust 1.88 CI matrix, CodeQL, RustSec, repository audit, and
   package checks.
3. Run the clean-worktree gate:

   ```sh
   cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-check --state release-ready \
     --notes-file target/xtask-package-check/release-notes.md
   ```

4. Confirm that source and packaged provenance refer to the public
   `AilloliAI/ailloli_ui` repository.
5. Freeze the validated `main` revision. No source modification is permitted
   between green CI and tag creation.

The repository audit always requires `.github/FUNDING.yml` to contain the
canonical `AilloliAI` beneficiary. Sponsorship remains voluntary and is not a
dependency of crates.io publication.

## Tag the validated revision

Set `RELEASE_VERSION` from the exact Cargo workspace metadata and compare it to
the changelog before using the commands below. The local checkout must retain a
freshly fetched `refs/remotes/origin/main`, which the tagged check compares to
`HEAD`. After dedicated approval, create one annotated tag on the exact green
SHA:

```sh
test -n "${RELEASE_VERSION:?set RELEASE_VERSION from workspace metadata}"
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-check \
  --state release-ready \
  --notes-file target/xtask-package-check/release-notes.md &&
git tag -a "v${RELEASE_VERSION}" -m "Ailloli UI ${RELEASE_VERSION}"
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-check \
  --state tagged --tag "v${RELEASE_VERSION}" \
  --notes-file target/xtask-package-check/release-notes.md
```

Stop here for human review. Tag creation does not authorize a remote mutation.

## Push the reviewed tag

After separate approval naming the exact tag and commit, push only that tag:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-check \
  --state tagged --tag "v${RELEASE_VERSION}" \
  --notes-file target/xtask-package-check/release-notes.md &&
git push origin "v${RELEASE_VERSION}"
```

Do not push any other tag and do not modify the tag after publication. The tag
workflow must succeed on the exact tagged SHA, and its tagged release check must
pass again, before any crate upload begins.

## Publish the crates sequentially

Publication is manual unless a separately reviewed and approved workflow says
otherwise. Provide `CARGO_REGISTRY_TOKEN` only in the ephemeral environment of
the approved publication commands. Keep it outside Git, logs, shell history,
and release artifacts, and do not use `cargo login`. Follow the exact levels
printed by:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-plan
```

For each crate in one level, wait until its first-party dependencies resolve
from crates.io, set `CRATE` to its exact package name, and run from a clean
public checkout detached at the release tag:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu xtask package-check --package "${CRATE}"
cargo +1.88.0-x86_64-unknown-linux-gnu publish --locked --dry-run --package "${CRATE}"
cargo +1.88.0-x86_64-unknown-linux-gnu publish --locked --package "${CRATE}"
```

The selected package check uses normal lockfile handling and records the
publish-equivalent archive in `target/xtask-package-check/publication-ledger.json`.
Run the real command only after that crate's dry-run succeeds, then wait until
the version is resolvable from crates.io before advancing to a dependent level.
Publish `ailloli_ui` last.

After an ambiguous response or timeout, query crates.io before retrying. Never
upload a different archive under the same version. If publication stops after
some crates succeed, resume only the missing crates from the same tag and
unchanged archives.

## Verify registry state and create the pre-release

Verification requires the publish-equivalent ledger produced by selected
package checks, a clean public checkout at the exact annotated tag, that tag on
`HEAD`, and the unchanged local archives described by the ledger. The preflight
manifest is never accepted as publication evidence.
The verifier rereads each local archive and checks its size and SHA-256 before
comparing it with crates.io. After the final upload, verify the entire
synchronized set:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu xtask verify-release --version "$RELEASE_VERSION"
```

`--version` may be omitted to use the workspace version. Keeping it explicit in
the post-publication command makes the registry target reviewable.

For a safe partial retry or focused diagnosis, repeat `--package` as needed:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu xtask verify-release --version "$RELEASE_VERSION" \
  --package ailloli_ui_core \
  --package ailloli_ui
```

The verification covers version, yank state, checksum, repository,
documentation, license, Rust version, README, and normalized first-party
requirements. Also compile an external consumer using only the registry and
wait for every docs.rs page required by the release.

Dependency verification uses the official Cargo sparse index, where `name` is
the manifest alias and `package` is the original crate name. The REST dependency
response may omit that alias and is not sufficient to attest renamed edges.
The verifier selects exactly one entry for the requested version, checks its
checksum and yank state, and compares every first-party requirement, alias,
kind, target, optional flag, default-feature flag, and enabled feature with the
frozen archive. Missing, duplicate, malformed, or unsupported index entries
fail validation; an absent alias is never silently accepted.

Exercise both default and no-default registry consumers, plus representative
optional integrations. Keep their manifests and lockfiles outside the source
checkout and preserve the resolved registry versions as validation evidence.
When verifying an older tag with corrected tooling, run the reviewed verifier
binary with `--root` pointing to the clean tagged checkout and its original
publication ledger. Record the verifier revision or source hash separately;
do not patch the historical checkout or regenerate the published archives.

Only after all 22 crates and the external consumer are verified may a maintainer
create the matching GitHub pre-release on the existing tag. Use the reviewed
output of `cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-notes` as its
body. Recheck the preserved file with `release-notes --check` immediately before
passing it to `gh release create --notes-file`. Do not substitute GitHub's
generated notes, the tag annotation, a summary, or a changelog link. Do not let
the release action create, move, or replace the tag.

After publication, retrieve the actual GitHub Release `body` through the API
and save it as `target/xtask-package-check/published-release-notes.md`. Verify it
with the same gate from the tagged checkout:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-notes \
  --version "${RELEASE_VERSION}" \
  --check target/xtask-package-check/published-release-notes.md
```

Also verify the tag, commit, pre-release status and rendered Markdown. An empty
or truncated GitHub body leaves publication validation incomplete, even when
the tag and crates are already available.

After the canonical release URL has been verified, candidate links may be
updated in a separately reviewed documentation change. This link-only follow-up
must not change the tag, archives, crate versions, or published release notes.

If a later historical review finds an omission or inaccurate description in
published notes, correct the current changelog editorially using evidence from
that version's immutable tag. Preserve the release heading and date, keep the
reviewed original body and its hash, and do not attribute later code to the old
release. An authorized notes-only correction may update the GitHub body without
moving the tag. Compare it with the reviewed editorial text, not with an older
tag's unchanged extraction, and record the correction under `[Unreleased]` in
`Project`. Never add a new validation tool to an old tag merely to check its notes.

For post-release maintenance, run the affected tests, repository audit, archive
preflight, and `release-notes --check` against the reviewed historical body.
Do not run `release-check` or dispatch the release workflow as a maintenance
gate: those commands validate a new freeze or the exact tagged source, require
an empty `[Unreleased]` after the version is finalized, and must reject pending
changes for that same version. Keep maintenance entries under `[Unreleased]`
and leave existing tags and published packages unchanged.

## Ownership and later automation

A separately approved operation may add an organization team as crate owner or
configure Trusted Publishing. Those operations are independent of a source
release and must not be inferred from release approval.

Any future automated publication workflow must publish synchronized releases
only from an explicitly approved version tag. It must never publish from an
ordinary push to `main`.

## Rollback and correction

If a candidate is invalid before a tag is pushed, prepare a corrected commit and
discard only the unpushed local tag. If a published version is invalid, document
it as withdrawn or request a separately authorized yank, then prepare a new
pre-release version. Never replace already distributed source.
