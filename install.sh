#!/usr/bin/env sh
# Meng installer: makes `meng` available in your shell.
#
#   curl -sSf https://raw.githubusercontent.com/simenglabs/mengs-lang/main/install.sh | sh
#   curl -sSf .../install.sh | sh -s -- --vscode         also install the VS Code extension
#   curl -sSf .../install.sh | sh -s -- --from-source    build with Rust instead of downloading
#
#   ./install.sh [--vscode] [--from-source]             from a local checkout (always builds)
#
# By default a prebuilt binary is downloaded from GitHub Releases into ~/.meng/bin
# (no Rust needed). If that is not possible, Meng is built from source with cargo.
# Environment: MENG_VERSION=v0.1.0 to pin a release, MENG_RELEASE_URL for a mirror,
# MENG_REPO for another source repo.
set -e

GITHUB="https://github.com/simenglabs/mengs-lang"
REPO_URL="${MENG_REPO:-$GITHUB.git}"
VERSION="${MENG_VERSION:-latest}"

USER_PATH="$PATH"   # PATH as the user's shell sees it (before we touch it)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info() { printf '\033[1;34m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[1;33m[WARN]\033[0m %s\n' "$1"; }
fail() { printf '\033[1;31m[ERROR]\033[0m %s\n' "$1" >&2; exit 1; }

VSCODE=0
FROM_SOURCE=0
for arg in "$@"; do
  case "$arg" in
    --vscode) VSCODE=1 ;;
    --from-source) FROM_SOURCE=1 ;;
    *) fail "Unknown option: $arg (use --vscode and/or --from-source)" ;;
  esac
done

# A local checkout (the script run as a file next to Cargo.toml) always builds from it.
ROOT=""
if [ -f "$0" ] && [ -f "$(dirname "$0")/Cargo.toml" ]; then
  ROOT="$(cd "$(dirname "$0")" && pwd)"
  FROM_SOURCE=1
fi

if [ -n "$MENG_RELEASE_URL" ]; then
  RELEASE_URL="$MENG_RELEASE_URL" # a mirror (or a local folder for testing)
elif [ "$VERSION" = latest ]; then
  RELEASE_URL="$GITHUB/releases/latest/download"
else
  RELEASE_URL="$GITHUB/releases/download/$VERSION"
fi

download() { # url file
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$1" -O "$2"
  else
    return 1
  fi
}

# Release asset name for this machine, e.g. meng-aarch64-apple-darwin.tar.gz.
target() {
  case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) return 1 ;;
  esac
  case "$(uname -s)" in
    Darwin) echo "$arch-apple-darwin" ;;
    Linux) echo "$arch-unknown-linux-musl" ;;
    *) return 1 ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# 1. Prebuilt binary ------------------------------------------------------------
install_prebuilt() {
  TARGET="$(target)" || { warn "No prebuilt binary for $(uname -s) $(uname -m)."; return 1; }
  ARCHIVE="meng-$TARGET.tar.gz"
  info "Downloading $ARCHIVE ($VERSION)..."
  download "$RELEASE_URL/$ARCHIVE" "$TMP/$ARCHIVE" || { warn "Could not download $RELEASE_URL/$ARCHIVE"; return 1; }
  if download "$RELEASE_URL/$ARCHIVE.sha256" "$TMP/$ARCHIVE.sha256"; then
    expected="$(cut -d' ' -f1 < "$TMP/$ARCHIVE.sha256")"
    actual="$(sha256 "$TMP/$ARCHIVE")"
    [ -z "$actual" ] || [ "$expected" = "$actual" ] || fail "Checksum mismatch for $ARCHIVE."
  fi
  tar -xzf "$TMP/$ARCHIVE" -C "$TMP" || return 1
  BIN_DIR="$HOME/.meng/bin"
  mkdir -p "$BIN_DIR"
  mv "$TMP/meng" "$BIN_DIR/meng"
  chmod +x "$BIN_DIR/meng"
}

# 2. Build from source ------------------------------------------------------------
install_from_source() {
  BIN_DIR="$HOME/.cargo/bin"
  if ! command -v cargo >/dev/null 2>&1; then
    [ -x "$BIN_DIR/cargo" ] && PATH="$BIN_DIR:$PATH" || fail "Rust is not installed. Install it with:
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
and run this installer again."
  fi
  if [ -z "$ROOT" ]; then
    command -v git >/dev/null 2>&1 || fail "git is required to download the Meng source."
    info "Downloading Meng source from $REPO_URL..."
    git clone --quiet --depth 1 "$REPO_URL" "$TMP/src" || fail "Could not clone $REPO_URL"
    ROOT="$TMP/src"
  fi
  info "Building and installing meng..."
  cargo install --path "$ROOT" --force --quiet
}

if [ "$FROM_SOURCE" = 1 ] || ! install_prebuilt; then
  [ "$FROM_SOURCE" = 1 ] || info "Falling back to building from source."
  install_from_source
fi
command -v gcc >/dev/null 2>&1 || warn "gcc not found: 'meng interpret' works, but 'meng compile/run' needs gcc."

# 3. Make sure the bin folder is on PATH -------------------------------------------
case ":$USER_PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    case "$(basename "${SHELL:-sh}")" in
      zsh)  RC="$HOME/.zshrc" ;;
      bash) [ "$(uname)" = Darwin ] && RC="$HOME/.bash_profile" || RC="$HOME/.bashrc" ;;
      *)    RC="$HOME/.profile" ;;
    esac
    LINE="export PATH=\"\$HOME/${BIN_DIR#"$HOME"/}:\$PATH\""
    if ! grep -qsF "$LINE" "$RC"; then
      printf '\n# Added by the Meng installer\n%s\n' "$LINE" >> "$RC"
      info "Added ${BIN_DIR#"$HOME"/} to PATH in $RC"
    fi
    NEED_RELOAD="$RC"
    ;;
esac

# 4. Optional: VS Code extension --------------------------------------------------
if [ "$VSCODE" = 1 ]; then
  CODE="$(command -v code || true)"
  MAC_CODE="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
  [ -z "$CODE" ] && [ -x "$MAC_CODE" ] && CODE="$MAC_CODE"
  [ -n "$CODE" ] || fail "VS Code CLI ('code') not found."

  info "Installing the VS Code extension..."
  VSIX="$TMP/meng-lang.vsix"
  if [ -n "$ROOT" ]; then
    sh "$ROOT/editors/vscode/package.sh" "$VSIX" >/dev/null
  else
    download "$RELEASE_URL/meng-lang.vsix" "$VSIX" || fail "Could not download $RELEASE_URL/meng-lang.vsix"
  fi
  "$CODE" --install-extension "$VSIX" --force >/dev/null 2>&1 || fail "VS Code could not install the extension."
  info "VS Code extension installed (reload VS Code to activate it)."
fi

# 5. Done --------------------------------------------------------------------------
info "Installed: $BIN_DIR/meng"
if [ -n "$NEED_RELOAD" ]; then
  echo "    Open a new terminal (or run: source $NEED_RELOAD), then try:"
else
  echo "    Try:"
fi
echo "      meng            # show usage"
echo "      meng interpret hello.meng"
