#!/usr/bin/env sh
# Meng installer: builds the compiler and makes `meng` available in your shell.
#
#   curl -sSf https://raw.githubusercontent.com/simenglabs/mengs-lang/main/install.sh | sh
#   curl -sSf .../install.sh | sh -s -- --vscode   also install the VS Code extension
#
#   ./install.sh [--vscode]                        from a local checkout
set -e

# Where to fetch the source from when the script is piped from curl.
REPO_URL="${MENG_REPO:-https://github.com/simenglabs/mengs-lang.git}"

BIN_DIR="$HOME/.cargo/bin"
USER_PATH="$PATH"   # PATH as the user's shell sees it (before we touch it)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info() { printf '\033[1;34m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[1;33m[WARN]\033[0m %s\n' "$1"; }
fail() { printf '\033[1;31m[ERROR]\033[0m %s\n' "$1" >&2; exit 1; }

# 1. Requirements -------------------------------------------------------------
if ! command -v cargo >/dev/null 2>&1; then
  [ -x "$BIN_DIR/cargo" ] && PATH="$BIN_DIR:$PATH" || fail "Rust is not installed. Install it with:
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
and run this installer again."
fi
command -v gcc >/dev/null 2>&1 || warn "gcc not found: 'meng interpret' works, but 'meng compile/run' needs gcc."

# 2. Get the source -----------------------------------------------------------
# Run as a file inside a checkout -> use it. Piped from curl -> clone the repo.
if [ -f "$0" ] && [ -f "$(dirname "$0")/Cargo.toml" ]; then
  ROOT="$(cd "$(dirname "$0")" && pwd)"
else
  command -v git >/dev/null 2>&1 || fail "git is required to download Meng."
  info "Downloading Meng from $REPO_URL..."
  git clone --quiet --depth 1 "$REPO_URL" "$TMP/src" || fail "Could not clone $REPO_URL"
  ROOT="$TMP/src"
fi

# 3. Build and install --------------------------------------------------------
info "Building and installing meng..."
cargo install --path "$ROOT" --force --quiet

# 4. Make sure ~/.cargo/bin is on PATH ----------------------------------------
case ":$USER_PATH:" in
  *":$BIN_DIR:"*) ;;
  *)
    case "$(basename "${SHELL:-sh}")" in
      zsh)  RC="$HOME/.zshrc" ;;
      bash) [ "$(uname)" = Darwin ] && RC="$HOME/.bash_profile" || RC="$HOME/.bashrc" ;;
      *)    RC="$HOME/.profile" ;;
    esac
    LINE='export PATH="$HOME/.cargo/bin:$PATH"'
    if ! grep -qsF "$LINE" "$RC"; then
      printf '\n# Added by the Meng installer\n%s\n' "$LINE" >> "$RC"
      info "Added ~/.cargo/bin to PATH in $RC"
    fi
    NEED_RELOAD="$RC"
    ;;
esac

# 5. Optional: VS Code extension ----------------------------------------------
if [ "$1" = "--vscode" ]; then
  CODE="$(command -v code || true)"
  MAC_CODE="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
  [ -z "$CODE" ] && [ -x "$MAC_CODE" ] && CODE="$MAC_CODE"
  [ -n "$CODE" ] || fail "VS Code CLI ('code') not found."
  command -v zip >/dev/null 2>&1 || fail "'zip' is required to package the extension."

  info "Installing the VS Code extension..."
  EXT="$ROOT/editors/vscode"
  VERSION="$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' "$EXT/package.json" | head -n 1)"
  mkdir -p "$TMP/pkg/extension"
  cp -R "$EXT/package.json" "$EXT/extension.js" "$EXT/symbols.js" \
        "$EXT/language-configuration.json" "$EXT/syntaxes" "$EXT/snippets" \
        "$EXT/README.md" "$TMP/pkg/extension/"

  cat > "$TMP/pkg/extension.vsixmanifest" <<EOF
<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011">
  <Metadata>
    <Identity Language="en-US" Id="meng-lang" Version="$VERSION" Publisher="menglabs" />
    <DisplayName>Meng Language</DisplayName>
    <Description xml:space="preserve">Meng language support</Description>
    <Properties><Property Id="Microsoft.VisualStudio.Code.Engine" Value="^1.75.0" /></Properties>
  </Metadata>
  <Installation><InstallationTarget Id="Microsoft.VisualStudio.Code"/></Installation>
  <Dependencies/>
  <Assets><Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true" /></Assets>
</PackageManifest>
EOF
  cat > "$TMP/pkg/[Content_Types].xml" <<'EOF'
<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension=".json" ContentType="application/json"/>
  <Default Extension=".js" ContentType="application/javascript"/>
  <Default Extension=".md" ContentType="text/markdown"/>
  <Default Extension=".vsixmanifest" ContentType="text/xml"/>
</Types>
EOF
  (cd "$TMP/pkg" && zip -qr "$TMP/meng-lang.vsix" .)
  "$CODE" --install-extension "$TMP/meng-lang.vsix" --force >/dev/null 2>&1 \
    || fail "VS Code could not install the extension."
  info "VS Code extension installed (reload VS Code to activate it)."
fi

# 6. Done ----------------------------------------------------------------------
info "Installed: $BIN_DIR/meng"
if [ -n "$NEED_RELOAD" ]; then
  echo "    Open a new terminal (or run: source $NEED_RELOAD), then try:"
else
  echo "    Try:"
fi
echo "      meng            # show usage"
echo "      meng interpret hello.meng"
