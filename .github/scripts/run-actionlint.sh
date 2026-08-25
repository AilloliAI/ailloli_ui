#!/usr/bin/env bash
set -euo pipefail

readonly ACTIONLINT_VERSION="1.7.12"
readonly ACTIONLINT_ARCHIVE_SHA256="8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8"
readonly ACTIONLINT_ARCHIVE="actionlint_${ACTIONLINT_VERSION}_linux_amd64.tar.gz"
readonly ACTIONLINT_URL="https://github.com/rhysd/actionlint/releases/download/v${ACTIONLINT_VERSION}/${ACTIONLINT_ARCHIVE}"

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
fi

actual_version="$(${binary} -version | sed -n '1p')"
if [[ "${actual_version}" != "${ACTIONLINT_VERSION}" ]]; then
  printf 'actionlint version mismatch: expected %s, got %s\n' \
    "${ACTIONLINT_VERSION}" "${actual_version}" >&2
  exit 1
fi

exec "${binary}" "$@"
