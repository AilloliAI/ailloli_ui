# Migrating Ailloli UI consumers

Ailloli UI is pre-1.0. Compatibility changes are documented here so the README
can remain focused on the current framework surface.

## Cargo feature migration

Earlier development snapshots exposed kebab-case aliases for first-party Cargo
features. Those aliases have been removed. Update consumer manifests and
`--features` arguments by replacing each complete legacy name with its
snake_case counterpart:

| Legacy | Current | Legacy | Current |
| --- | --- | --- | --- |
| `desktop-calibration` | `desktop_calibration` | `devtools-terminal` | `devtools_terminal` |
| `files-local` | `files_local` | `full-local` | `full_local` |
| `linux-portal-input` | `linux_portal_input` | `mock-transport` | `mock_transport` |
| `native-overlay` | `native_overlay` | `openssh-sftp` | `openssh_sftp` |
| `remote-openssh-sftp` | `remote_openssh_sftp` | `remote-sftp` | `remote_sftp` |
| `remote-sftp-vendored-openssl` | `remote_sftp_vendored_openssl` | `smoke-ui` | `smoke_ui` |
| `ssh-exec` | `ssh_exec` | `terminal-portable` | `terminal_portable` |
| `terminal-pty` | `terminal_pty` | `terminal-pty-portable` | `terminal_pty_portable` |
| `test-support` | `test_support` | `tree-sitter` | `tree_sitter` |
| `tree-sitter-bash` | `tree_sitter_bash` | `tree-sitter-c` | `tree_sitter_c` |
| `tree-sitter-css` | `tree_sitter_css` | `tree-sitter-go` | `tree_sitter_go` |
| `tree-sitter-html` | `tree_sitter_html` | `tree-sitter-java` | `tree_sitter_java` |
| `tree-sitter-javascript` | `tree_sitter_javascript` | `tree-sitter-json` | `tree_sitter_json` |
| `tree-sitter-markdown` | `tree_sitter_markdown` | `tree-sitter-php` | `tree_sitter_php` |
| `tree-sitter-python` | `tree_sitter_python` | `tree-sitter-ruby` | `tree_sitter_ruby` |
| `tree-sitter-swift` | `tree_sitter_swift` | `tree-sitter-toml` | `tree_sitter_toml` |
| `tree-sitter-typescript` | `tree_sitter_typescript` | `tree-sitter-yaml` | `tree_sitter_yaml` |
| `vendored-openssl` | `vendored_openssl` | `wgpu-target` | `wgpu_target` |

For example:

```toml
[dependencies]
ailloli_ui = { path = "crates/ailloli_ui", features = ["files_local", "tree_sitter"] }
```

```sh
cargo check --features files_local,tree_sitter
```

Upstream Cargo package names such as `tree-sitter-*`, `raw-window-handle`, and
`openssh-sftp-client` are unchanged. Human-facing CLI binaries also remain
`ailloli-ui-bench` and `cargo-ailloli-ui`.

For the current feature responsibilities and dependency boundaries, see
[ARCHITECTURE.md](ARCHITECTURE.md).
