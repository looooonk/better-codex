#!/bin/sh

set -eu

repository="looooonk/better-codex"
version=${BETTER_CODEX_RELEASE:-}
archive_path=${BETTER_CODEX_ARCHIVE_PATH:-}
release_index_url="https://raw.githubusercontent.com/$repository/main/scripts/latest-release"
github_releases_url="https://api.github.com/repos/$repository/releases?per_page=1"

usage() {
    cat <<'EOF'
Install Better Codex from a GitHub release.

Usage: install.sh [--version VERSION]

Environment:
  BETTER_CODEX_INSTALL_ROOT  Release storage directory
  BETTER_CODEX_BIN_DIR       Directory for the better-codex launcher
  BETTER_CODEX_RELEASE       Release version, equivalent to --version
  BETTER_CODEX_ARCHIVE_PATH  Local archive for package testing
  GH_TOKEN/GITHUB_TOKEN      GitHub token for fallback release discovery
EOF
}

fail() {
    printf 'better-codex installer: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || fail "--version requires a value"
            version=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown argument: $1"
            ;;
    esac
done

command -v tar >/dev/null 2>&1 || fail "tar is required"

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-aarch64|Linux-arm64) target=aarch64-unknown-linux-musl ;;
    Linux-x86_64|Linux-amd64) target=x86_64-unknown-linux-musl ;;
    *) fail "unsupported platform: $(uname -s) $(uname -m)" ;;
esac

download() {
    url=$1
    destination=$2
    if command -v curl >/dev/null 2>&1; then
        curl -fL --retry 3 --connect-timeout 15 -o "$destination" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$destination" "$url"
    else
        fail "curl or wget is required"
    fi
}

download_github_release_metadata() {
    destination=$1
    headers=$2
    github_token=${GH_TOKEN:-${GITHUB_TOKEN:-}}
    status=

    if command -v curl >/dev/null 2>&1; then
        if [ -n "$github_token" ]; then
            status=$(curl -sSL --connect-timeout 15 \
                -D "$headers" \
                -H "Accept: application/vnd.github+json" \
                -H "Authorization: Bearer $github_token" \
                -H "X-GitHub-Api-Version: 2022-11-28" \
                -o "$destination" \
                -w '%{http_code}' \
                "$github_releases_url") || return 1
        else
            status=$(curl -sSL --connect-timeout 15 \
                -D "$headers" \
                -H "Accept: application/vnd.github+json" \
                -H "X-GitHub-Api-Version: 2022-11-28" \
                -o "$destination" \
                -w '%{http_code}' \
                "$github_releases_url") || return 1
        fi
    elif command -v wget >/dev/null 2>&1; then
        if [ -n "$github_token" ]; then
            wget --server-response --tries=1 --timeout=15 \
                --header="Accept: application/vnd.github+json" \
                --header="Authorization: Bearer $github_token" \
                --header="X-GitHub-Api-Version: 2022-11-28" \
                -O "$destination" "$github_releases_url" 2>"$headers" || true
        else
            wget --server-response --tries=1 --timeout=15 \
                --header="Accept: application/vnd.github+json" \
                --header="X-GitHub-Api-Version: 2022-11-28" \
                -O "$destination" "$github_releases_url" 2>"$headers" || true
        fi
        status=$(sed -n \
            's/^[[:space:]]*HTTP\/[^ ]*[[:space:]]*\([0-9][0-9][0-9]\).*/\1/p' \
            "$headers" | tail -n 1)
    else
        fail "curl or wget is required"
    fi

    case "$status" in
        2??) return 0 ;;
    esac

    remaining=$(sed -n \
        's/^[[:space:]]*[Xx]-[Rr]ate[Ll]imit-[Rr]emaining:[[:space:]]*//p' \
        "$headers" | tail -n 1 | tr -d '\r')
    reset=$(sed -n \
        's/^[[:space:]]*[Xx]-[Rr]ate[Ll]imit-[Rr]eset:[[:space:]]*//p' \
        "$headers" | tail -n 1 | tr -d '\r')
    if { [ "$status" = 403 ] || [ "$status" = 429 ]; } && [ "$remaining" = 0 ]; then
        if [ -n "$reset" ]; then
            printf 'better-codex installer: GitHub API rate limit exhausted; resets at Unix time %s\n' \
                "$reset" >&2
        else
            printf '%s\n' \
                'better-codex installer: GitHub API rate limit exhausted' >&2
        fi
        if [ -z "$github_token" ]; then
            printf '%s\n' \
                'better-codex installer: set GH_TOKEN or GITHUB_TOKEN to authenticate the fallback request' >&2
        fi
    else
        printf 'better-codex installer: GitHub release request failed with HTTP %s\n' \
            "${status:-unknown}" >&2
    fi
    return 1
}

if [ -z "$version" ]; then
    [ -z "$archive_path" ] || fail "set --version when installing a local archive"
    metadata_dir=$(mktemp -d "${TMPDIR:-/tmp}/better-codex-metadata.XXXXXX")
    metadata="$metadata_dir/release"
    headers="$metadata_dir/headers"
    trap 'rm -rf "$metadata_dir"' EXIT HUP INT TERM
    if download "$release_index_url" "$metadata" 2>/dev/null; then
        version=$(sed -n '1{s/[[:space:]]*$//;p;}' "$metadata")
    else
        download_github_release_metadata "$metadata" "$headers" || \
            fail "could not determine the latest release; pass --version VERSION to bypass release discovery"
        tag=$(sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' "$metadata")
        case "$tag" in
            v*) version=${tag#v} ;;
            *) fail "latest release tag is not a Better Codex version" ;;
        esac
    fi
    rm -rf "$metadata_dir"
    trap - EXIT HUP INT TERM
fi

case "$version" in
    v*) version=${version#v} ;;
esac
case "$version" in
    *[!0-9A-Za-z.+-]*|'') fail "invalid release version: $version" ;;
esac

install_root=${BETTER_CODEX_INSTALL_ROOT:-${XDG_DATA_HOME:-"$HOME/.local/share"}/better-codex}
bin_dir=${BETTER_CODEX_BIN_DIR:-"$HOME/.local/bin"}
asset="better-codex-package-$target.tar.gz"
temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/better-codex.XXXXXX")
stage=
cleanup() {
    rm -rf "$temporary_dir"
    if [ -n "$stage" ] && [ -d "$stage" ]; then
        rm -rf "$stage"
    fi
}
trap cleanup EXIT HUP INT TERM
archive="$temporary_dir/$asset"

if [ -n "$archive_path" ]; then
    cp "$archive_path" "$archive"
else
    release_url="https://github.com/$repository/releases/download/v$version"
    download "$release_url/$asset" "$archive"
    download "$release_url/$asset.sha256" "$temporary_dir/$asset.sha256"
    expected=$(sed -n '1{s/[[:space:]].*//;p;}' "$temporary_dir/$asset.sha256")
    [ -n "$expected" ] || fail "release checksum is empty"
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$archive" | sed 's/[[:space:]].*//')
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$archive" | sed 's/[[:space:]].*//')
    else
        fail "sha256sum or shasum is required"
    fi
    [ "$actual" = "$expected" ] || fail "release checksum verification failed"
fi

tar -tzf "$archive" | while IFS= read -r entry; do
    case "$entry" in
        /*|../*|*/../*) fail "archive contains an unsafe path: $entry" ;;
    esac
done

mkdir -p "$install_root/releases" "$bin_dir"
stage=$(mktemp -d "$install_root/releases/.install.XXXXXX")
tar -xzf "$archive" -C "$stage" --strip-components 1
[ -x "$stage/bin/codex" ] || fail "archive does not contain bin/codex"
[ -x "$stage/bin/codex-code-mode-host" ] || fail "archive does not contain code-mode host"
installed_version=$("$stage/bin/codex" --version)
case "$installed_version" in
    *" $version") ;;
    *) fail "archive contains version '$installed_version', expected $version" ;;
esac

release_dir="$install_root/releases/$version-$target"
if [ -e "$release_dir" ]; then
    rm -rf "$stage"
else
    mv "$stage" "$release_dir"
fi
stage=

current_link="$install_root/.current.$$"
ln -s "$release_dir" "$current_link"
case "$target" in
    aarch64-apple-darwin|x86_64-apple-darwin)
        mv -fh "$current_link" "$install_root/current"
        ;;
    aarch64-unknown-linux-musl|x86_64-unknown-linux-musl)
        mv -fT "$current_link" "$install_root/current"
        ;;
esac

active_version=$("$install_root/current/bin/codex" --version)
case "$active_version" in
    *" $version") ;;
    *) fail "activated version '$active_version', expected $version" ;;
esac

escaped_root=$(printf '%s' "$install_root" | sed "s/'/'\\\\''/g")
launcher="$bin_dir/.better-codex.$$"
{
    printf '%s\n' '#!/bin/sh' 'set -eu'
    printf "install_root=\${BETTER_CODEX_INSTALL_ROOT:-'%s'}\n" "$escaped_root"
    # shellcheck disable=SC2016
    printf '%s\n' \
        'export BETTER_CODEX_MANAGED=1' \
        'export BETTER_CODEX_INSTALL_ROOT="$install_root"' \
        'export CODEX_CODE_MODE_HOST_PATH="$install_root/current/bin/codex-code-mode-host"' \
        'export PATH="$install_root/current/codex-path:$PATH"' \
        'exec "$install_root/current/bin/codex" "$@"'
} >"$launcher"
chmod 755 "$launcher"
mv -f "$launcher" "$bin_dir/better-codex"

printf 'Better Codex %s installed to %s\n' "$version" "$bin_dir/better-codex"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) printf 'Add %s to PATH before running better-codex.\n' "$bin_dir" ;;
esac
