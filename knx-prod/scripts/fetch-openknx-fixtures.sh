#!/usr/bin/env bash
# Fetch OpenKNX release XMLs for integration testing.
# These files are large (50-65MB) and not stored in the repository.
#
# Usage: ./scripts/fetch-openknx-fixtures.sh

set -euo pipefail

FIXTURE_DIR="$(cd "$(dirname "$0")/../tests/fixtures/openknx" && pwd -P 2>/dev/null || echo "$(dirname "$0")/../tests/fixtures/openknx")"
mkdir -p "$FIXTURE_DIR"

fetch() {
    local repo=$1 name=$2 xml_path=$3
    local target="$FIXTURE_DIR/$name.xml"

    if [ -f "$target" ]; then
        echo "✓ $name.xml (cached)"
        return
    fi

    echo "⬇ Fetching $repo latest release..."
    local url
    url=$(curl -sL "https://api.github.com/repos/OpenKNX/$repo/releases/latest" \
        | grep -o '"browser_download_url": "[^"]*"' | head -1 | cut -d'"' -f4)

    if [ -z "$url" ]; then
        echo "  ✗ No release found for $repo"
        return 1
    fi

    local tmpzip
    tmpzip=$(mktemp)
    curl -sL "$url" -o "$tmpzip"
    # Handle both Unix and Windows path separators in ZIP
    unzip -o "$tmpzip" -d "$FIXTURE_DIR" 2>/dev/null || true
    local found
    found=$(find "$FIXTURE_DIR" -name "$xml_path" -type f 2>/dev/null | head -1)
    if [ -n "$found" ]; then
        mv "$found" "$target"
        echo "  ✓ $name.xml ($(du -h "$target" | cut -f1))"
    else
        echo "  ✗ $xml_path not found in archive"
    fi
    # Clean up extracted dirs
    rm -rf "$FIXTURE_DIR/data" "$FIXTURE_DIR/ETS-Applikation" "$tmpzip"
}

echo "Fetching OpenKNX fixtures into $FIXTURE_DIR"
echo ""
fetch OAM-SmartHomeBridge SmartHomeBridge SmartHomeBridge.xml
fetch OAM-LogicModule     LogicModule     LogicModule.xml
echo ""
echo "Done. Run integration tests with:"
echo "  cargo test -p knx-prod --test openknx"
