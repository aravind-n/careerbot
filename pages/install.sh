#!/bin/sh
# careerbot installer.
#
# Resolves the latest GitHub release, downloads the binary that
# matches this host's OS+arch, and installs it to
# $CAREERBOT_INSTALL_DIR (default $HOME/.local/bin).
#
# Usage (from the hosted copy):
#   curl -fsSL https://aravind-n.github.io/careerbot/install.sh | sh
#
# Environment:
#   CAREERBOT_VERSION       pin a release tag (e.g. v0.1.0). Default: latest.
#   CAREERBOT_INSTALL_DIR   install destination. Default: $HOME/.local/bin.
#
# Supported platforms: linux-x86_64, macos-aarch64. Anything else is
# refused — the CI release pipeline does not produce other binaries.

set -eu

REPO="aravind-n/careerbot"
INSTALL_DIR="${CAREERBOT_INSTALL_DIR:-$HOME/.local/bin}"

die() {
    printf 'careerbot-install: %s\n' "$1" >&2
    exit 1
}

need() {
    command -v "$1" >/dev/null 2>&1 || die "missing required tool: $1"
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Linux)  os_tag=linux  ;;
        Darwin) os_tag=macos  ;;
        *) die "unsupported OS: $os (need Linux or Darwin)" ;;
    esac
    case "$arch" in
        x86_64|amd64)  arch_tag=x86_64  ;;
        arm64|aarch64) arch_tag=aarch64 ;;
        *) die "unsupported arch: $arch" ;;
    esac
    case "$os_tag-$arch_tag" in
        linux-x86_64|macos-aarch64) printf '%s-%s\n' "$os_tag" "$arch_tag" ;;
        *) die "no release binary for $os_tag-$arch_tag (supported: linux-x86_64, macos-aarch64)" ;;
    esac
}

resolve_tag() {
    if [ -n "${CAREERBOT_VERSION:-}" ]; then
        printf '%s\n' "$CAREERBOT_VERSION"
        return
    fi
    api_url="https://api.github.com/repos/$REPO/releases/latest"
    tag=$(curl -fsSL "$api_url" \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        | head -n 1)
    [ -n "$tag" ] || die "could not resolve latest release tag from $api_url"
    printf '%s\n' "$tag"
}

main() {
    need curl
    need uname
    need mktemp
    need install

    target=$(detect_target)
    tag=$(resolve_tag)
    asset="careerbot-${tag}-${target}"
    url="https://github.com/$REPO/releases/download/$tag/$asset"

    printf 'careerbot %s for %s\n' "$tag" "$target"
    printf '  source: %s\n' "$url"
    printf '  dest:   %s/careerbot\n' "$INSTALL_DIR"

    tmpdir=$(mktemp -d 2>/dev/null || mktemp -d -t careerbot)
    trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM

    curl -fSL --progress-bar "$url" -o "$tmpdir/careerbot" \
        || die "download failed: $url"

    mkdir -p "$INSTALL_DIR" || die "cannot create install dir: $INSTALL_DIR"
    install -m 0755 "$tmpdir/careerbot" "$INSTALL_DIR/careerbot" \
        || die "cannot install to $INSTALL_DIR"

    printf 'installed: %s/careerbot\n' "$INSTALL_DIR"

    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            printf '\n'
            printf 'note: %s is not on your PATH. Add to your shell rc:\n' "$INSTALL_DIR"
            printf '    export PATH="%s:$PATH"\n' "$INSTALL_DIR"
            ;;
    esac
}

main "$@"
