#!/usr/bin/env bash
# Downloads the NSH 0.6.0 embedding SDKs that the iOS and Android host examples
# link, checks each archive against the sha256 pinned below (and against the
# release's own .sha256 file), and unpacks them into the git-ignored
# .nsh-embed/0.6.0/ at the repository root:
#
#   .nsh-embed/0.6.0/ios/{NativeShell,NativeShellDev}.xcframework
#   .nsh-embed/0.6.0/android/{nsh,nsh-dev}.aar
#
# From 0.5.0 on, NSH's release archives no longer carry these SDKs and
# `nsh component` has no embed component; the release publishes them alone as
# nsh-embed-ios.zip and nsh-embed-android.zip. Homebrew's formula still ships
# 0.4.0, so its libexec/embed is not used.
#
# Usage: tools/fetch-nsh-embed.sh [ios|android|all]   (default: all)
# Prints the directory of each SDK it ensured. Idempotent: an SDK already
# unpacked from the pinned archive is not downloaded again.
# NSH_EMBED_ROOT overrides where the SDKs are unpacked (default: <repo>/.nsh-embed).
set -euo pipefail

NSH_VERSION=0.6.0
BASE_URL="https://github.com/longbridge/nsh/releases/download/v$NSH_VERSION"
SHA256_IOS=5d34b2feaee92c813eb12afe3d928f346a020988531efaf94b5d4d124f0b0afd
SHA256_ANDROID=fc59ffa013fc43265924a0e55cca58bb66e17e1c011b2d640e7cd43fb7c4a492

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${NSH_EMBED_ROOT:-$ROOT/.nsh-embed}/$NSH_VERSION"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/nsh-embed.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# $1 platform (ios|android), $2 pinned sha256, $3 a file the SDK must contain.
fetch() {
    local platform="$1" sha="$2" must="$3"
    local zip="nsh-embed-$platform.zip" out="$DEST/$platform"
    if [ -f "$out/.sha256" ] && [ "$(cat "$out/.sha256")" = "$sha" ] && [ -e "$out/$must" ]; then
        echo "$out"
        return
    fi
    local work="$WORK/$platform"
    mkdir -p "$work"
    echo "fetch-nsh-embed: downloading $BASE_URL/$zip" >&2
    # The release's own checksum must name the pinned hash, and the archive must match it.
    curl -fsSL --retry 3 -o "$work/$zip.sha256" "$BASE_URL/$zip.sha256"
    published="$(awk '{print $1; exit}' "$work/$zip.sha256")"
    [ "$published" = "$sha" ] || {
        echo "fetch-nsh-embed: $zip.sha256 says $published, pinned is $sha" >&2
        exit 1
    }
    curl -fsSL --retry 3 -o "$work/$zip" "$BASE_URL/$zip"
    (cd "$work" && echo "$sha  $zip" | shasum -a 256 -c - >&2)
    unzip -q "$work/$zip" -d "$work/x"
    [ -e "$work/x/nsh-embed-$platform/$must" ] || {
        echo "fetch-nsh-embed: $zip has no nsh-embed-$platform/$must" >&2
        exit 1
    }
    rm -rf "$out"
    mkdir -p "$DEST"
    mv "$work/x/nsh-embed-$platform" "$out"
    echo "$sha" > "$out/.sha256"
    echo "$out"
}

case "${1:-all}" in
    ios) fetch ios "$SHA256_IOS" NativeShell.xcframework/Info.plist ;;
    android) fetch android "$SHA256_ANDROID" nsh.aar ;;
    all)
        fetch ios "$SHA256_IOS" NativeShell.xcframework/Info.plist
        fetch android "$SHA256_ANDROID" nsh.aar
        ;;
    *)
        echo "usage: $0 [ios|android|all]" >&2
        exit 2
        ;;
esac
