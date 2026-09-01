# RustSec Triage

The public beta gate uses `cargo-audit 0.22.2` under Rust 1.88 against the
committed `Cargo.lock`. Vulnerabilities fail the gate. New informational,
unsound, unmaintained, or yanked warnings also fail unless their exact advisory
ID is reviewed here and added to `.cargo/audit.toml`.

The review snapshot below was produced on 2026-08-24. Every entry must be
revisited before a beta release and whenever its direct dependency chain
changes.

## Resolved vulnerability

`RUSTSEC-2026-0253` affected `lru::LruCache::pop()` before `0.18.2`. The
workspace now resolves exactly `lru 0.18.2`, the first fixed version. This ID is
not ignored; a lockfile regression will fail `cargo audit`.

## Temporarily accepted informational advisories

### RUSTSEC-2024-0436: `paste 1.0.15` unmaintained

`paste` is a build-time procedural macro reached through `image -> exr -> pulp`
and `image -> ravif -> rav1e`. The advisory reports maintenance status, not a
known vulnerability. A direct replacement is not controlled by this workspace;
the chain will be reviewed when `image`, `exr`, or `ravif` changes.

Disposition: temporarily accepted with no runtime API exposure. Remove the
exception when upstream moves to a maintained macro implementation, or
immediately if a vulnerability advisory is issued.

### RUSTSEC-2026-0206: `rustybuzz 0.20.1` unmaintained

`rustybuzz` is selected by `usvg 0.47`, which is shared by SVG validation and
rasterization through `resvg`. The advisory reports that the crate is
unmaintained and recommends `harfrust`; it does not identify a vulnerability.
Changing the shaping implementation belongs to a reviewed `usvg`/`resvg`
upgrade rather than an unscoped lockfile rewrite.

Disposition: temporarily accepted while retaining bounded SVG validation.
Re-evaluate on every `usvg`/`resvg` update and remove the exception when that
chain adopts a maintained shaper.

### RUSTSEC-2026-0192: `ttf-parser 0.21.1` and `0.25.1` unmaintained

Version `0.21.1` is selected by `fontdue 0.9`; version `0.25.1` is used directly
and by `fontdb`, `owned_ttf_parser`, `rustybuzz`, and `usvg`. The advisory is an
unmaintained notice without a patched version and recommends `skrifa`.
Replacing both versions spans font discovery, layout, SVG, icon, and native
window dependencies and requires separate compatibility and visual testing.

Disposition: temporarily accepted. Continue bounded font and SVG inputs,
review upstream migrations, and remove the exception once all required chains
can move to a maintained parser without changing the public API accidentally.

### RUSTSEC-2026-0186: `memmap2 0.8.0` unsound range methods

The affected version is present only in the all-features Wayland chain
`smithay-client-toolkit 0.19.2 -> xkbcommon 0.7.0 -> memmap2 0.8.0`.
`xkbcommon` maps a keymap with `MmapOptions::map_copy_read_only`; it does not
call any affected `advise_range`, `unchecked_advise_range`, `flush_range`, or
`flush_async_range` method. A fixed `memmap2 0.9.11` is also present elsewhere,
but the `xkbcommon` compatibility constraint prevents a lockfile-only update.

Disposition: temporarily accepted because the affected API is not invoked by
the selected chain. Remove the exception when the Wayland dependency accepts
`memmap2 >=0.9.11`, or immediately if the call graph or advisory scope changes.

## Review procedure

For each candidate, run `cargo tree --workspace --all-features -i
<package>@<version>`, read the current RustSec advisory, verify the affected
features and functions, and update this file before changing the ignore list.
An ignore entry without a matching section here is a gate failure.
