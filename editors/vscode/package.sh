#!/usr/bin/env sh
# Package the VS Code extension into a .vsix without npm or network access.
#
#   editors/vscode/package.sh [output.vsix]     (default: meng-lang.vsix here)
#
# A .vsix is a zip with a manifest; this builds the same layout `vsce package` does.
set -e

EXT="$(cd "$(dirname "$0")" && pwd)"
OUT="${1:-$EXT/meng-lang.vsix}"
case "$OUT" in /*) ;; *) OUT="$(pwd)/$OUT" ;; esac
command -v zip >/dev/null 2>&1 || { echo "[ERROR] 'zip' is required to package the extension." >&2; exit 1; }

field() { sed -n "s/.*\"$1\": *\"\([^\"]*\)\".*/\1/p" "$EXT/package.json" | head -n 1; }
VERSION="$(field version)"
PUBLISHER="$(field publisher)"
NAME="$(field name)"
DISPLAY="$(field displayName)"
DESCRIPTION="$(field description)"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/extension"
cp -R "$EXT/package.json" "$EXT/extension.js" "$EXT/symbols.js" "$EXT/language-configuration.json" \
      "$EXT/syntaxes" "$EXT/snippets" "$EXT/README.md" "$TMP/extension/"
[ -f "$EXT/icon.png" ] && cp "$EXT/icon.png" "$TMP/extension/"

cat > "$TMP/extension.vsixmanifest" <<MANIFEST
<?xml version="1.0" encoding="utf-8"?>
<PackageManifest Version="2.0.0" xmlns="http://schemas.microsoft.com/developer/vsx-schema/2011">
  <Metadata>
    <Identity Language="en-US" Id="$NAME" Version="$VERSION" Publisher="$PUBLISHER" />
    <DisplayName>$DISPLAY</DisplayName>
    <Description xml:space="preserve">$DESCRIPTION</Description>
    <Properties><Property Id="Microsoft.VisualStudio.Code.Engine" Value="^1.75.0" /></Properties>
  </Metadata>
  <Installation><InstallationTarget Id="Microsoft.VisualStudio.Code"/></Installation>
  <Dependencies/>
  <Assets><Asset Type="Microsoft.VisualStudio.Code.Manifest" Path="extension/package.json" Addressable="true" /></Assets>
</PackageManifest>
MANIFEST
cat > "$TMP/[Content_Types].xml" <<'TYPES'
<?xml version="1.0" encoding="utf-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension=".json" ContentType="application/json"/>
  <Default Extension=".js" ContentType="application/javascript"/>
  <Default Extension=".md" ContentType="text/markdown"/>
  <Default Extension=".png" ContentType="image/png"/>
  <Default Extension=".vsixmanifest" ContentType="text/xml"/>
</Types>
TYPES

rm -f "$OUT"
(cd "$TMP" && zip -qr "$OUT" .)
echo "$OUT"
