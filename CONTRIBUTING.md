# Contributing to Ailloli UI

Thank you for helping improve Ailloli UI. The project welcomes focused bug
fixes, tests, documentation, performance work, and framework features that
preserve its application-neutral boundaries.

## Before you start

- Use Rust 1.88 and the toolchain declared by the repository.
- Search existing issues before opening a new one.
- Discuss broad API or architectural changes before investing in an
  implementation.
- Report suspected vulnerabilities through the private process in
  [SECURITY.md](SECURITY.md), never through an issue or pull request.
- Do not include credentials, customer data, private source, generated build
  output, or machine-specific paths.

## Repository boundaries

The Cargo workspace is autonomous. Framework packages must not depend on a
consumer application. The `sandbox_app` package is a public consumer example
and depends directly only on the `ailloli_ui` façade. The non-publishable
`xtask` package validates the repository and release artifacts without
depending on framework crates.

Keep provider selection, business policy, credentials, and product-specific
workflows in consuming applications. Public code, documentation, fixtures, and
commit messages must not expose private repository names or local paths.

## Punctuation

Em dashes are not permitted in first-party public text, documentation, UI
labels, source comments, Rustdoc, fixtures, reports, or commit messages. Use a
colon after a title, label, term, or tier name that introduces a description.
Use parentheses or commas for a parenthetical remark, and use a semicolon or a
new sentence for contrast, qualification, or consequence. Use an en dash or
write the range explicitly for ranges and intervals.
Never replace a range with a colon. Preserve third-party legal text verbatim.

## Development workflow

Create a small branch from `main`, keep each change reviewable, and add tests
for observable behavior. Public APIs require Rustdoc that explains contracts,
errors, safety boundaries, and a runnable example when practical. Prefer
deterministic tests; native browser, network, or filesystem side effects must
not occur implicitly.

Run from the repository root:

```sh
cargo +1.88.0-x86_64-unknown-linux-gnu metadata --locked --format-version 1
cargo +1.88.0-x86_64-unknown-linux-gnu fmt --all -- --check
cargo +1.88.0-x86_64-unknown-linux-gnu check --workspace --all-targets --all-features --locked
cargo +1.88.0-x86_64-unknown-linux-gnu test --workspace --all-features --locked
cargo +1.88.0-x86_64-unknown-linux-gnu test --workspace --doc --all-features --locked
cargo +1.88.0-x86_64-unknown-linux-gnu clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo +1.88.0-x86_64-unknown-linux-gnu doc --workspace --all-features --no-deps --locked
cargo +1.88.0-x86_64-unknown-linux-gnu check -p ailloli_ui --no-default-features --locked
cargo +1.88.0-x86_64-unknown-linux-gnu xtask audit
cargo +1.88.0-x86_64-unknown-linux-gnu xtask package-check --allow-dirty
```

Run the repository audit and RustSec audit described by CI as well. Visual UI
changes need a deterministic capture and human inspection for hierarchy,
clipping, focus, overlays, and theme consistency.

## Pull requests and review

A pull request should explain the problem, the chosen boundary, test evidence,
and any user-visible or compatibility impact. Keep formatting-only or generated
changes separate from behavioral work. Review may request smaller commits,
additional negative tests, documentation, or a migration note.

The project uses review rather than automatic merge. Passing CI is necessary
but does not replace maintainer review. Do not rewrite another contributor's
work or add a sponsor, logo, endorsement, or testimonial without permission.

## Licensing

Unless explicitly stated otherwise, contributions are accepted under either
Apache License 2.0 or the MIT License, at the user's option. By submitting a
contribution, you represent that you have the right to license it under those
terms. Preserve third-party notices and provenance for imported assets or code.
