#!/bin/sh
# agentspaces installer: downloads the latest asp release binary for this
# platform, verifies its checksum, and installs it to ~/.local/bin (or
# ASP_INSTALL_DIR). No sudo, no telemetry.
set -eu

REPO="ArnavBorkar/agentspaces"
INSTALL_DIR="${ASP_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*" >&2; }
fail() { say "error: $*"; exit 1; }

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v git >/dev/null 2>&1 || say "warning: git not found — asp requires git >= 2.30 at runtime"

os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-gnu" ;;
  *) fail "unsupported OS: $os (build from source: cargo install --git https://github.com/$REPO asp)" ;;
esac
case "$arch" in
  arm64|aarch64) arch="aarch64" ;;
  x86_64|amd64) arch="x86_64" ;;
  *) fail "unsupported architecture: $arch" ;;
esac
target="$arch-$os"

say "resolving latest release…"
tag="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep '"tag_name"' | head -1 | cut -d '"' -f4)" || true
[ -n "${tag:-}" ] || fail "no published release found yet — build from source: cargo install --git https://github.com/$REPO asp"

asset="asp-$tag-$target.tar.gz"
url="https://github.com/$REPO/releases/download/$tag/$asset"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

say "downloading $asset…"
curl -fsSL "$url" -o "$tmp/$asset" || fail "download failed: $url"
curl -fsSL "$url.sha256" -o "$tmp/$asset.sha256" || fail "checksum download failed"

say "verifying checksum…"
expected="$(cut -d ' ' -f1 < "$tmp/$asset.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$asset" | cut -d ' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f1)"
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch (expected $expected, got $actual)"

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
