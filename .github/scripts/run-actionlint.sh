#!/usr/bin/env bash
set -euo pipefail

readonly ACTIONLINT_VERSION="1.7.12"
readonly ACTIONLINT_ARCHIVE_SHA256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
readonly ACTIONLINT_ARCHIVE="actionlint_${ACTIONLINT_VERSION}_linux_amd64.tar.gz"
readonly ACTIONLINT_URL="https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/${ACTIONLINT_ARCHIVE}"

isolated_root=""
if [[ "${1:-}" == "--isolated-root" ]]; then
  if (($# != 2)); then
    printf '%s\n' \
      "usage: run-actionlint.sh --isolated-root PATH" >&2
    exit 2
  fi
  isolated_root="$2"
  shift 2
fi

cache_base="${XDG_CACHE_HOME:-${HOME}/.cache}/ailloli-ui/actionlint/${ACTIONLINT_VERSION}"
binary="${ACTIONLINT_BIN:-${cache_base}/actionlint}"

if [[ ! -x "${binary}" ]]; then
  mkdir -p "${cache_base}"
  temp_dir="$(mktemp -d "${cache_base}/install.XXXXXXXX")"
  trap 'rm -rf -- "${temp_dir}"' EXIT
  curl --fail --location --proto '=https' --tlsv1.2 \
    --output "${temp_dir}/${ACTIONLINT_ARCHIVE}" "${ACTIONLINT_URL}"
  printf '%s  %s\n' "${ACTIONLINT_ARCHIVE_SHA256}" \
    "${temp_dir}/${ACTIONLINT_ARCHIVE}" | sha256sum --check --status
  tar --extract --gzip --file "${temp_dir}/${ACTIONLINT_ARCHIVE}" \
    --directory "${temp_dir}" actionlint
  install -m 0755 "${temp_dir}/actionlint" "${binary}"
  rm -rf -- "${temp_dir}"
  trap - EXIT
fi

actual_version="$(${binary} -version | sed -n '1p')"
if [[ "${actual_version}" != "${ACTIONLINT_VERSION}" ]]; then
  printf 'actionlint version mismatch: expected %s, got %s\n' \
    "${ACTIONLINT_VERSION}" "${actual_version}" >&2
  exit 1
fi

if [[ -n "${isolated_root}" ]]; then
  if [[ ! -d "${isolated_root}/.github/workflows" ]]; then
    printf 'isolated workflow root is missing: %s\n' "${isolated_root}" >&2
    exit 2
  fi
  isolated_root="$(cd -- "${isolated_root}" && pwd -P)"
  isolated_temp="$(mktemp -d "${TMPDIR:-/tmp}/ailloli_actionlint.XXXXXXXX")"
  trap 'rm -rf -- "${isolated_temp}"' EXIT
  git init --quiet --initial-branch=main "${isolated_temp}"
  mkdir -p "${isolated_temp}/.github"
  cp -a -- "${isolated_root}/.github/workflows" \
    "${isolated_temp}/.github/workflows"
  if find "${isolated_temp}/.github/workflows" -type l -print -quit | grep -q .; then
    printf '%s\n' "isolated workflows must not contain symlinks" >&2
    exit 1
  fi
  (
    cd -- "${isolated_temp}"
    "${binary}" .github/workflows/*.yml
  )
  exit 0
fi

exec "${binary}" "$@"
