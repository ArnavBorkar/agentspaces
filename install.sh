#!/bin/sh
# agentspaces installer: downloads the latest asp release binary for this
# platform, verifies its checksum, and installs it to ~/.local/bin (or
# ASP_INSTALL_DIR). No sudo, no telemetry.
set -eu

REPO="ArnavBorkar/agentspaces"
INSTALL_DIR="${ASP_INSTALL_DIR:-$HOME/.local/bin}"
CURL="${ASP_CURL:-curl}"

say() { printf '%s\n' "$*" >&2; }
fail() {
  say "error: $1"
  if [ "${2:-}" ]; then
    say "hint: $2"
  fi
  exit 1
}

command -v "$CURL" >/dev/null 2>&1 || fail "curl is required" "install curl, or set ASP_CURL to a compatible downloader"
command -v git >/dev/null 2>&1 || say "warning: git not found — asp requires git >= 2.32 at runtime"

os="${ASP_INSTALL_OS:-$(uname -s)}"
arch="${ASP_INSTALL_ARCH:-$(uname -m)}"
case "$os" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *) fail "unsupported OS: $os" "supported installers: macOS and Linux; otherwise build from source with: cargo install --git https://github.com/$REPO asp" ;;
esac
case "$arch" in
  arm64|aarch64) arch="aarch64" ;;
  x86_64|amd64) arch="x86_64" ;;
  *) fail "unsupported architecture: $arch" "supported architectures: x86_64 and aarch64" ;;
esac
target="$arch-$os"

if [ "${ASP_INSTALL_VERSION:-}" ]; then
  tag="$ASP_INSTALL_VERSION"
else
  say "resolving latest release..."
  release_json="$("$CURL" -fsSL "https://api.github.com/repos/$REPO/releases/latest")" \
    || fail "could not reach GitHub releases API" "check your network/proxy, set ASP_INSTALL_VERSION to a released tag, or build from source with: cargo install --git https://github.com/$REPO asp"
  tag="$(printf '%s\n' "$release_json" | grep '"tag_name"' | head -1 | cut -d '"' -f4)" || true
  [ -n "${tag:-}" ] || fail "no published release found yet" "set ASP_INSTALL_VERSION to a released tag, or build from source with: cargo install --git https://github.com/$REPO asp"
fi

asset="asp-$tag-$target.tar.gz"
url="https://github.com/$REPO/releases/download/$tag/$asset"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "downloading ${asset}..."
"$CURL" -fsSL "$url" -o "$tmp/$asset" \
  || fail "download failed: $url" "check network/proxy settings, verify the release exists for $target, or build from source"
"$CURL" -fsSL "$url.sha256" -o "$tmp/$asset.sha256" \
  || fail "checksum download failed: $url.sha256" "do not run the archive without a checksum; retry or verify the release manually"

say "verifying checksum..."
expected="$(cut -d ' ' -f1 < "$tmp/$asset.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$asset" | cut -d ' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f1)"
fi
[ "$expected" = "$actual" ] \
  || fail "checksum mismatch (expected $expected, got $actual)" "do not run this archive; retry the download, and report the release if the mismatch persists"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$tmp/asp" "$INSTALL_DIR/asp"

say ""
say "✓ asp $tag installed to $INSTALL_DIR/asp"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) say "  add it to your PATH:  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
say "  get started:          cd your-project && asp init"
say "  wire up Claude Code:  asp setup claude"
