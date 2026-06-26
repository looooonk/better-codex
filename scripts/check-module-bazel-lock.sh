#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"
if ! bazel mod deps --lockfile_mode=error; then
  echo "MODULE.bazel.lock is out of date."
  echo "Run 'just bazel-lock-update' and commit the updated lockfile."
  exit 1
fi
