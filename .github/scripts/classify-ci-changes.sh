#!/usr/bin/env bash
set -euo pipefail

readonly ZERO_SHA="0000000000000000000000000000000000000000"
readonly SCRIPT_PATH="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)/$(basename -- "${BASH_SOURCE[0]}")"

usage() {
  cat <<'EOF'
Usage: classify-ci-changes.sh [options]

Classify a Git change range for the Ailloli UI Internal or Public workspace.

Options:
  --repo PATH                 Git repository to inspect (default: current repo)
  --public-prefix PATH        Public workspace prefix; empty for Public
  --event NAME                GitHub event name
  --before SHA                Push before SHA
  --base SHA                  Pull request or merge-group base SHA
  --head SHA                  Candidate head SHA (default: HEAD)
  --pr-head SHA               Pull request head SHA
  --ref-type TYPE             GitHub ref type (branch or tag)
  --expected-profile NAME     Assert the resulting closed profile
  --replay NAME               Build and classify a bounded canary fixture
  --self-test                 Run the isolated Git fixture suite
  --help                      Show this help

The script appends stable Boolean outputs to GITHUB_OUTPUT when it is set.
EOF
}

repo=""
public_prefix="__auto__"
event_name="${CI_EVENT_NAME:-${GITHUB_EVENT_NAME:-}}"
before_sha="${CI_BEFORE_SHA:-}"
base_sha="${CI_BASE_SHA:-}"
head_sha="${CI_HEAD_SHA:-${GITHUB_SHA:-HEAD}}"
pr_head_sha="${CI_PR_HEAD_SHA:-}"
ref_type="${CI_REF_TYPE:-${GITHUB_REF_TYPE:-}}"
expected_profile=""
replay=""
self_test=false

while (($# > 0)); do
  case "$1" in
    --repo)
      repo="${2:?--repo requires a path}"
      shift 2
      ;;
    --public-prefix)
      public_prefix="${2-}"
      shift 2
      ;;
    --event)
      event_name="${2:?--event requires a value}"
      shift 2
      ;;
    --before)
      before_sha="${2:?--before requires a SHA}"
      shift 2
      ;;
    --base)
      base_sha="${2:?--base requires a SHA}"
      shift 2
      ;;
    --head)
      head_sha="${2:?--head requires a SHA}"
      shift 2
      ;;
    --pr-head)
      pr_head_sha="${2:?--pr-head requires a SHA}"
      shift 2
      ;;
    --ref-type)
      ref_type="${2:?--ref-type requires a value}"
      shift 2
      ;;
    --expected-profile)
      expected_profile="${2:?--expected-profile requires a value}"
      shift 2
      ;;
    --replay)
      replay="${2:?--replay requires a value}"
      shift 2
      ;;
    --self-test)
      self_test=true
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'classify-ci-changes: ERROR: unsupported argument: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

bool() {
  if [[ "$1" == true ]]; then
    printf 'true'
  else
    printf 'false'
  fi
}

init_fixture_repo() {
  local root="$1"
  git init --quiet --initial-branch=main "$root"
  git -C "$root" config user.name "Ailloli CI fixture"
  git -C "$root" config user.email "ci-fixture@example.invalid"
  mkdir -p \
    "$root/.agent" \
    "$root/.github/workflows" \
    "$root/framework/assets" \
    "$root/framework/crates/demo/src" \
    "$root/framework/crates/demo/assets" \
    "$root/internal" \
    "$root/scripts"
  printf '# Context\n' > "$root/.agent/context.md"
  printf 'name: current\n' > "$root/.github/workflows/ci.yml"
  printf '# Framework\n' > "$root/framework/README.md"
  printf '# Security\n' > "$root/framework/SECURITY.md"
  printf '[workspace]\nmembers = ["crates/demo"]\n' > "$root/framework/Cargo.toml"
  printf '# lock fixture\n' > "$root/framework/Cargo.lock"
  printf 'banner-v1\n' > "$root/framework/assets/ailloli_ui_banner.png"
  printf '[package]\nname = "demo"\nversion = "0.0.0"\n' \
    > "$root/framework/crates/demo/Cargo.toml"
  printf '# Demo\n' > "$root/framework/crates/demo/README.md"
  printf 'pub fn fixture() {}\n' > "$root/framework/crates/demo/src/lib.rs"
  printf 'asset-v1\n' > "$root/framework/crates/demo/assets/runtime.bin"
  printf 'version = 1\n' > "$root/internal/public-manifest.toml"
  printf '#!/usr/bin/env bash\nexit 0\n' > "$root/scripts/public-audit.sh"
  git -C "$root" add --all
  git -C "$root" commit --quiet -m "fixture: baseline"
}

fixture_commit() {
  local root="$1"
  local message="$2"
  git -C "$root" add --all
  git -C "$root" commit --quiet -m "$message"
}

run_fixture_profile() {
  local name="$1"
  local expected="$2"
  local mutate="$3"
  local temp_root fixture before head
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_fixture.XXXXXXXX")"
  fixture="$temp_root/repo"
  init_fixture_repo "$fixture"
  before="$(git -C "$fixture" rev-parse HEAD)"

  case "$mutate" in
    readme)
      printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
      ;;
    private_context)
      printf 'updated\n' >> "$fixture/.agent/context.md"
      ;;
    docs_brand)
      printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
      printf 'banner-v2\n' > "$fixture/framework/assets/ailloli_ui_banner.png"
      ;;
    governance)
      printf 'Private reporting only.\n' >> "$fixture/framework/SECURITY.md"
      ;;
    workflow_policy)
      printf 'on: workflow_dispatch\n' >> "$fixture/.github/workflows/ci.yml"
      ;;
    package)
      printf 'Crate documentation.\n' >> "$fixture/framework/crates/demo/README.md"
      ;;
    dependencies)
      printf '\n[workspace.package]\nversion = "0.0.1"\n' >> "$fixture/framework/Cargo.toml"
      printf '# updated lock\n' >> "$fixture/framework/Cargo.lock"
      ;;
    rust)
      printf 'pub fn changed() {}\n' >> "$fixture/framework/crates/demo/src/lib.rs"
      ;;
    source_deleted)
      rm -- "$fixture/framework/crates/demo/src/lib.rs"
      ;;
    rust_to_markdown)
      git -C "$fixture" mv \
        framework/crates/demo/src/lib.rs \
        framework/crates/demo/NOTES.md
      ;;
    markdown_to_rust)
      git -C "$fixture" mv \
        framework/crates/demo/README.md \
        framework/crates/demo/src/readme.rs
      ;;
    crate_asset)
      printf 'asset-v2\n' > "$fixture/framework/crates/demo/assets/runtime.bin"
      ;;
    unknown)
      printf 'unknown\n' > "$fixture/framework/unclassified.payload"
      ;;
    mixed_docs_private)
      printf 'updated\n' >> "$fixture/.agent/context.md"
      printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
      ;;
    rename_delete)
      git -C "$fixture" mv \
        framework/crates/demo/src/lib.rs \
        framework/crates/demo/NOTES.md
      rm -- "$fixture/framework/assets/ailloli_ui_banner.png"
      ;;
    first_of_multi)
      printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
      ;;
    copy)
      cp -- "$fixture/framework/crates/demo/src/lib.rs" \
        "$fixture/framework/crates/demo/COPY.md"
      ;;
    *)
      printf 'classify-ci-changes: ERROR: unknown fixture mutation %s\n' "$mutate" >&2
      rm -rf -- "$temp_root"
      return 1
      ;;
  esac

  fixture_commit "$fixture" "fixture: $name"
  head="$(git -C "$fixture" rev-parse HEAD)"
  env -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
    "$SCRIPT_PATH" \
      --repo "$fixture" \
      --public-prefix framework \
      --event push \
      --before "$before" \
      --head "$head" \
      --expected-profile "$expected" >/dev/null
  rm -rf -- "$temp_root"
  printf 'classify-ci-changes: ok: %s\n' "$name"
}

run_root_fixture() {
  local temp_root fixture head
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_root.XXXXXXXX")"
  fixture="$temp_root/repo"
  git init --quiet --initial-branch=main "$fixture"
  git -C "$fixture" config user.name "Ailloli CI fixture"
  git -C "$fixture" config user.email "ci-fixture@example.invalid"
  mkdir -p "$fixture/framework"
  printf '# Root documentation\n' > "$fixture/framework/README.md"
  git -C "$fixture" add --all
  git -C "$fixture" commit --quiet -m "fixture: root"
  head="$(git -C "$fixture" rev-parse HEAD)"
  env -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
    "$SCRIPT_PATH" \
      --repo "$fixture" \
      --public-prefix framework \
      --event push \
      --head "$head" \
      --expected-profile docs_brand >/dev/null
  rm -rf -- "$temp_root"
  printf 'classify-ci-changes: ok: root commit\n'
}

run_zero_or_invalid_fixture() {
  local mode="$1"
  local temp_root fixture head before expected
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_range.XXXXXXXX")"
  fixture="$temp_root/repo"
  init_fixture_repo "$fixture"
  head="$(git -C "$fixture" rev-parse HEAD)"
  if [[ "$mode" == zero ]]; then
    before="$ZERO_SHA"
    expected=unknown_mixed
  else
    before="1111111111111111111111111111111111111111"
    expected=unknown_full
  fi
  env -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
    "$SCRIPT_PATH" \
      --repo "$fixture" \
      --public-prefix framework \
      --event push \
      --before "$before" \
      --head "$head" \
      --expected-profile "$expected" >/dev/null
  rm -rf -- "$temp_root"
  printf 'classify-ci-changes: ok: %s range\n' "$mode"
}

run_empty_fixture() {
  local temp_root fixture head
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_empty.XXXXXXXX")"
  fixture="$temp_root/repo"
  init_fixture_repo "$fixture"
  head="$(git -C "$fixture" rev-parse HEAD)"
  env -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
    "$SCRIPT_PATH" \
      --repo "$fixture" \
      --public-prefix framework \
      --event push \
      --before "$head" \
      --head "$head" \
      --expected-profile unknown_full >/dev/null
  rm -rf -- "$temp_root"
  printf 'classify-ci-changes: ok: empty range\n'
}

run_multi_commit_fixture() {
  local temp_root fixture before head
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_multi.XXXXXXXX")"
  fixture="$temp_root/repo"
  init_fixture_repo "$fixture"
  before="$(git -C "$fixture" rev-parse HEAD)"
  printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
  fixture_commit "$fixture" "fixture: docs"
  printf 'pub fn changed() {}\n' >> "$fixture/framework/crates/demo/src/lib.rs"
  fixture_commit "$fixture" "fixture: rust"
  head="$(git -C "$fixture" rev-parse HEAD)"
  env -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
    "$SCRIPT_PATH" \
      --repo "$fixture" \
      --public-prefix framework \
      --event push \
      --before "$before" \
      --head "$head" \
      --expected-profile rename_or_mixed >/dev/null
  rm -rf -- "$temp_root"
  printf 'classify-ci-changes: ok: multi-commit range\n'
}

run_event_fixture() {
  local name="$1"
  local event="$2"
  local expected="$3"
  local mutation="$4"
  local temp_root fixture baseline before head
  local -a arguments
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_event.XXXXXXXX")"
  fixture="$temp_root/repo"
  init_fixture_repo "$fixture"
  baseline="$(git -C "$fixture" rev-parse HEAD)"
  before="$baseline"
  head="$baseline"

  case "$mutation" in
    docs)
      printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
      fixture_commit "$fixture" "fixture: $name"
      head="$(git -C "$fixture" rev-parse HEAD)"
      ;;
    rust)
      printf 'pub fn event_change() {}\n' \
        >> "$fixture/framework/crates/demo/src/lib.rs"
      fixture_commit "$fixture" "fixture: $name"
      head="$(git -C "$fixture" rev-parse HEAD)"
      ;;
    none)
      ;;
    force_push)
      printf 'first branch documentation\n' >> "$fixture/framework/README.md"
      fixture_commit "$fixture" "fixture: discarded branch"
      before="$(git -C "$fixture" rev-parse HEAD)"
      git -C "$fixture" reset --quiet --hard "$baseline"
      printf 'replacement branch documentation\n' >> "$fixture/framework/README.md"
      fixture_commit "$fixture" "fixture: replacement branch"
      head="$(git -C "$fixture" rev-parse HEAD)"
      ;;
    *)
      printf 'classify-ci-changes: ERROR: unknown event mutation %s\n' \
        "$mutation" >&2
      rm -rf -- "$temp_root"
      return 1
      ;;
  esac

  arguments=(
    --repo "$fixture"
    --public-prefix framework
    --event "$event"
    --head "$head"
    --expected-profile "$expected"
  )
  case "$name" in
    "pull request"|"merge group")
      arguments+=(--base "$before" --pr-head "$head")
      ;;
    "tag push")
      arguments+=(--ref-type tag)
      ;;
    "force push")
      arguments+=(--before "$before")
      ;;
  esac

  env -u GITHUB_OUTPUT -u GITHUB_STEP_SUMMARY \
    "$SCRIPT_PATH" "${arguments[@]}" >/dev/null
  rm -rf -- "$temp_root"
  printf 'classify-ci-changes: ok: %s\n' "$name"
}

run_self_test() {
  run_fixture_profile "README" docs_brand readme
  run_fixture_profile "README and banner" docs_brand docs_brand
  run_fixture_profile "private context" private_context private_context
  run_fixture_profile "governance" docs_brand governance
  run_fixture_profile "workflow policy" workflow_policy workflow_policy
  run_fixture_profile "Cargo and lockfile" dependencies_full dependencies
  run_fixture_profile "Rust modification" rust_full rust
  run_fixture_profile "Rust deletion" rust_full source_deleted
  run_fixture_profile "Rust to Markdown rename" rename_or_mixed rust_to_markdown
  run_fixture_profile "Markdown to Rust rename" rename_or_mixed markdown_to_rust
  run_fixture_profile "crate runtime asset" rust_full crate_asset
  run_fixture_profile "crate README" package_docs package
  run_fixture_profile "unknown path" unknown_full unknown
  run_fixture_profile "mixed docs and private context" mixed_docs_private mixed_docs_private
  run_fixture_profile "copy detection" rename_or_mixed copy
  run_root_fixture
  run_zero_or_invalid_fixture zero
  run_zero_or_invalid_fixture invalid
  run_empty_fixture
  run_multi_commit_fixture
  run_event_fixture "pull request" pull_request docs_brand docs
  run_event_fixture "merge group" merge_group rust_full rust
  run_event_fixture "schedule" schedule full_only none
  run_event_fixture "manual dispatch" workflow_dispatch full_only none
  run_event_fixture "reusable call" workflow_call full_only none
  run_event_fixture "tag push" push full_only none
  run_event_fixture "force push" push unknown_docs force_push
  printf 'classify-ci-changes: PASS: 27 isolated Git scenarios\n'
}

prepare_replay() {
  local scenario="$1"
  local temp_root fixture before head expected mutation
  temp_root="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_replay.XXXXXXXX")"
  fixture="$temp_root/repo"
  init_fixture_repo "$fixture"
  before="$(git -C "$fixture" rev-parse HEAD)"

  case "$scenario" in
    private_context)
      expected=private_context
      mutation=private_context
      ;;
    docs_brand)
      expected=docs_brand
      mutation=docs_brand
      ;;
    workflow_policy)
      expected=workflow_policy
      mutation=workflow_policy
      ;;
    package)
      expected=package_docs
      mutation=package
      ;;
    dependencies|cargo_package)
      expected=dependencies_full
      mutation=dependencies
      ;;
    rust|release_ready)
      expected=rust_full
      mutation=rust
      ;;
    rename_delete)
      expected=rename_or_mixed
      mutation=rename_delete
      ;;
    fail_closed)
      head="$(git -C "$fixture" rev-parse HEAD)"
      "$SCRIPT_PATH" \
        --repo "$fixture" \
        --public-prefix framework \
        --event push \
        --before 1111111111111111111111111111111111111111 \
        --head "$head" \
        --expected-profile unknown_full
      rm -rf -- "$temp_root"
      return
      ;;
    *)
      printf 'classify-ci-changes: ERROR: unsupported replay: %s\n' "$scenario" >&2
      rm -rf -- "$temp_root"
      exit 2
      ;;
  esac

  case "$mutation" in
    private_context)
      printf 'updated\n' >> "$fixture/.agent/context.md"
      ;;
    docs_brand)
      printf 'Updated documentation.\n' >> "$fixture/framework/README.md"
      printf 'banner-v2\n' > "$fixture/framework/assets/ailloli_ui_banner.png"
      ;;
    workflow_policy)
      printf 'on: workflow_dispatch\n' >> "$fixture/.github/workflows/ci.yml"
      ;;
    package)
      printf 'Crate documentation.\n' >> "$fixture/framework/crates/demo/README.md"
      ;;
    dependencies)
      printf '\n[workspace.package]\nversion = "0.0.1"\n' >> "$fixture/framework/Cargo.toml"
      ;;
    rust)
      printf 'pub fn changed() {}\n' >> "$fixture/framework/crates/demo/src/lib.rs"
      ;;
    rename_delete)
      git -C "$fixture" mv \
        framework/crates/demo/src/lib.rs \
        framework/crates/demo/NOTES.md
      rm -- "$fixture/framework/assets/ailloli_ui_banner.png"
      ;;
  esac
  fixture_commit "$fixture" "fixture: replay $scenario"
  head="$(git -C "$fixture" rev-parse HEAD)"
  "$SCRIPT_PATH" \
    --repo "$fixture" \
    --public-prefix framework \
    --event push \
    --before "$before" \
    --head "$head" \
    --expected-profile "$expected"
  rm -rf -- "$temp_root"
}

if [[ "$self_test" == true ]]; then
  if [[ -n "$replay" ]]; then
    printf 'classify-ci-changes: ERROR: --self-test and --replay are exclusive\n' >&2
    exit 2
  fi
  run_self_test
  exit 0
fi

if [[ -n "$replay" ]]; then
  prepare_replay "$replay"
  exit 0
fi

if [[ -z "$repo" ]]; then
  repo="$(git rev-parse --show-toplevel 2>/dev/null || true)"
fi
if [[ -z "$repo" || ! -d "$repo" ]]; then
  printf 'classify-ci-changes: ERROR: a local Git repository is required\n' >&2
  exit 2
fi
repo="$(cd -- "$repo" && pwd -P)"
git_top="$(git -C "$repo" rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$git_top" || "$(cd -- "$git_top" && pwd -P)" != "$repo" ]]; then
  printf 'classify-ci-changes: ERROR: --repo must be a standalone Git root\n' >&2
  exit 2
fi

if [[ "$public_prefix" == __auto__ ]]; then
  if [[ -f "$repo/framework/Cargo.toml" && -d "$repo/internal" ]]; then
    public_prefix=framework
  else
    public_prefix=""
  fi
fi
public_prefix="${public_prefix#./}"
public_prefix="${public_prefix%/}"

private_context=false
docs_brand=false
workflow_policy=false
package=false
dependencies=false
rust=false
unknown_full=false
full=false
docs_only=false
declare -A changed_paths=()
declare -a changed_records=()

mark_unknown() {
  unknown_full=true
}

classify_public_path() {
  local path="$1"
  case "$path" in
    README.md|ARCHITECTURE.md|BENCHMARKING.md|CHANGELOG.md|CONTRIBUTING.md|MIGRATION.md|RELEASING.md|RUSTSEC.md|SECURITY.md|SPONSORS.md|SUPPORT.md|docs/*|artifacts/captures/*|assets/ailloli_ui_banner.png)
      docs_brand=true
      ;;
    LICENSE*|NOTICE)
      docs_brand=true
      package=true
      ;;
    .github/*)
      workflow_policy=true
      ;;
    .cargo/*|Cargo.toml|Cargo.lock|rust-toolchain|rust-toolchain.toml)
      dependencies=true
      package=true
      ;;
    .gitignore|.gitattributes|.editorconfig)
      workflow_policy=true
      package=true
      ;;
    crates/*/README.md|crates/*/*.md|crates/*/LICENSE*|crates/*/NOTICE)
      docs_brand=true
      package=true
      ;;
    crates/*/Cargo.toml)
      dependencies=true
      package=true
      ;;
    crates/*)
      rust=true
      package=true
      ;;
    apps/*/*.md|apps/*/README.md)
      docs_brand=true
      ;;
    apps/*/Cargo.toml)
      dependencies=true
      rust=true
      ;;
    apps/*)
      rust=true
      ;;
    tools/*/*.md|tools/*/README.md)
      docs_brand=true
      ;;
    tools/*/Cargo.toml)
      dependencies=true
      rust=true
      ;;
    tools/*)
      rust=true
      ;;
    .agent|.agent/*|.cursor|.cursor/*|internal|internal/*|scripts|scripts/*|AGENTS.md)
      mark_unknown
      ;;
    *)
      mark_unknown
      ;;
  esac
}

classify_path() {
  local path="${1#./}"
  if [[ -z "$path" || "$path" == /* ]]; then
    mark_unknown
    return
  fi

  if [[ -n "$public_prefix" ]]; then
    if [[ "$path" == "$public_prefix"/* ]]; then
      classify_public_path "${path#"$public_prefix"/}"
      return
    fi
    case "$path" in
      .agent|.agent/*|.cursor|.cursor/*|AGENTS.md)
        private_context=true
        ;;
      .github|.github/*|internal|internal/*|scripts|scripts/*)
        workflow_policy=true
        ;;
      *)
        mark_unknown
        ;;
    esac
    return
  fi

  classify_public_path "$path"
}

record_path() {
  local status="$1"
  local path="$2"
  changed_paths["$path"]=1
  changed_records+=("$status" "$path")
  classify_path "$path"
}

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_ci_diff.XXXXXXXX")"
trap 'rm -rf -- "$temp_dir"' EXIT
diff_file="$temp_dir/name-status.z"
range_ready=false

valid_commit() {
  git -C "$repo" rev-parse --verify --quiet "$1^{commit}" >/dev/null
}

write_diff() {
  if ! git -C "$repo" diff --name-status -z --find-renames --find-copies-harder \
    "$1" > "$diff_file"; then
    : > "$diff_file"
    mark_unknown
    return 1
  fi
  range_ready=true
}

if [[ "$ref_type" == tag ]]; then
  full=true
elif [[ "$event_name" == workflow_dispatch || "$event_name" == schedule || "$event_name" == workflow_call || "$event_name" == release ]]; then
  full=true
elif [[ "$event_name" == pull_request || "$event_name" == pull_request_review || "$event_name" == merge_group ]]; then
  candidate_head="${pr_head_sha:-$head_sha}"
  if [[ -n "$base_sha" ]] && valid_commit "$base_sha" && valid_commit "$candidate_head"; then
    write_diff "$base_sha...$candidate_head" || true
  else
    mark_unknown
  fi
elif [[ "$event_name" == push || -z "$event_name" ]]; then
  if ! valid_commit "$head_sha"; then
    mark_unknown
  elif [[ "$before_sha" == "$ZERO_SHA" ]]; then
    mark_unknown
    if git -C "$repo" diff-tree --root --no-commit-id --name-status -z \
      --find-renames --find-copies-harder -r "$head_sha" > "$diff_file"; then
      range_ready=true
    fi
  elif [[ -n "$before_sha" ]]; then
    if valid_commit "$before_sha"; then
      if ! git -C "$repo" merge-base --is-ancestor "$before_sha" "$head_sha"; then
        mark_unknown
      fi
      write_diff "$before_sha..$head_sha" || true
    else
      mark_unknown
    fi
  elif git -C "$repo" rev-parse --verify --quiet "$head_sha^" >/dev/null; then
    write_diff "$head_sha^..$head_sha" || true
  elif git -C "$repo" diff-tree --root --no-commit-id --name-status -z \
    --find-renames --find-copies-harder -r "$head_sha" > "$diff_file"; then
    range_ready=true
  else
    mark_unknown
  fi
else
  mark_unknown
fi

if [[ "$range_ready" == true ]]; then
  while IFS= read -r -d '' status; do
    case "$status" in
      R*|C*)
        if ! IFS= read -r -d '' old_path || ! IFS= read -r -d '' new_path; then
          mark_unknown
          break
        fi
        record_path "$status-old" "$old_path"
        record_path "$status-new" "$new_path"
        ;;
      A|D|M|T|U|X|B)
        if ! IFS= read -r -d '' path; then
          mark_unknown
          break
        fi
        record_path "$status" "$path"
        ;;
      *)
        mark_unknown
        break
        ;;
    esac
  done < "$diff_file"
fi

changed_count="${#changed_paths[@]}"
if [[ "$range_ready" == true && "$changed_count" -eq 0 && "$full" != true ]]; then
  mark_unknown
fi
if [[ "$dependencies" == true || "$rust" == true || "$unknown_full" == true ]]; then
  full=true
fi
if [[ "$docs_brand" == true && "$workflow_policy" != true && "$package" != true && "$dependencies" != true && "$rust" != true && "$unknown_full" != true ]]; then
  docs_only=true
fi

assert_value() {
  local label="$1"
  local actual="$2"
  local expected="$3"
  if [[ "$actual" != "$expected" ]]; then
    printf 'classify-ci-changes: ERROR: profile %s expected %s=%s, got %s\n' \
      "$expected_profile" "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

assert_profile() {
  local expected_private=false expected_docs=false expected_workflow=false
  local expected_package=false expected_dependencies=false expected_rust=false
  local expected_unknown=false expected_full=false expected_docs_only=false
  case "$expected_profile" in
    private_context)
      expected_private=true
      ;;
    docs_brand)
      expected_docs=true
      expected_docs_only=true
      ;;
    workflow_policy)
      expected_workflow=true
      ;;
    package_docs)
      expected_docs=true
      expected_package=true
      ;;
    dependencies_full)
      expected_package=true
      expected_dependencies=true
      expected_full=true
      ;;
    rust_full)
      expected_package=true
      expected_rust=true
      expected_full=true
      ;;
    rename_or_mixed)
      expected_docs=true
      expected_package=true
      expected_rust=true
      expected_full=true
      ;;
    mixed_docs_private)
      expected_private=true
      expected_docs=true
      expected_docs_only=true
      ;;
    unknown_full)
      expected_unknown=true
      expected_full=true
      ;;
    unknown_docs)
      expected_docs=true
      expected_unknown=true
      expected_full=true
      ;;
    full_only)
      expected_full=true
      ;;
    unknown_mixed)
      expected_private=true
      expected_docs=true
      expected_workflow=true
      expected_package=true
      expected_dependencies=true
      expected_rust=true
      expected_unknown=true
      expected_full=true
      ;;
    *)
      printf 'classify-ci-changes: ERROR: unknown expected profile: %s\n' \
        "$expected_profile" >&2
      exit 2
      ;;
  esac
  assert_value private_context "$private_context" "$expected_private"
  assert_value docs_brand "$docs_brand" "$expected_docs"
  assert_value workflow_policy "$workflow_policy" "$expected_workflow"
  assert_value package "$package" "$expected_package"
  assert_value dependencies "$dependencies" "$expected_dependencies"
  assert_value rust "$rust" "$expected_rust"
  assert_value unknown_full "$unknown_full" "$expected_unknown"
  assert_value full "$full" "$expected_full"
  assert_value docs_only "$docs_only" "$expected_docs_only"
}

if [[ -n "$expected_profile" ]]; then
  assert_profile
fi

emit_output() {
  local key="$1"
  local value="$2"
  printf '%s=%s\n' "$key" "$value"
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf '%s=%s\n' "$key" "$value" >> "$GITHUB_OUTPUT"
  fi
}

emit_output private_context "$(bool "$private_context")"
emit_output docs_brand "$(bool "$docs_brand")"
emit_output workflow_policy "$(bool "$workflow_policy")"
emit_output package "$(bool "$package")"
emit_output dependencies "$(bool "$dependencies")"
emit_output rust "$(bool "$rust")"
emit_output unknown_full "$(bool "$unknown_full")"
emit_output full "$(bool "$full")"
emit_output docs_only "$(bool "$docs_only")"
emit_output changed_count "$changed_count"

if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  {
    printf '## CI change classification\n\n'
    printf '| Scope | Active |\n|---|---|\n'
    printf '| private_context | `%s` |\n' "$private_context"
    printf '| docs_brand | `%s` |\n' "$docs_brand"
    printf '| workflow_policy | `%s` |\n' "$workflow_policy"
    printf '| package | `%s` |\n' "$package"
    printf '| dependencies | `%s` |\n' "$dependencies"
    printf '| rust | `%s` |\n' "$rust"
    printf '| unknown_full | `%s` |\n' "$unknown_full"
    printf '| full | `%s` |\n' "$full"
    printf '| docs_only | `%s` |\n' "$docs_only"
    printf '\nChanged paths: `%s`\n' "$changed_count"
    if ((${#changed_records[@]} > 0)); then
      printf '\n```text\n'
      for ((index = 0; index < ${#changed_records[@]}; index += 2)); do
        printf '%s ' "${changed_records[index]}"
        printf '%q\n' "${changed_records[index + 1]}"
      done
      printf '```\n'
    fi
  } >> "$GITHUB_STEP_SUMMARY"
fi
