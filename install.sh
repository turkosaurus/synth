#!/usr/bin/env bash
# Installs the latest stable synth for this machine:
#
#   curl -fsSL https://raw.githubusercontent.com/turkosaurus/synth/main/install.sh | bash
#
# Downloads the release archive, checks it against the sha256 digest GitHub
# recorded on upload, and runs `synth install`, which copies the binary to
# $XDG_BIN_HOME or ~/.local/bin. Arguments are passed to `synth install`.
# Needs bash 3.2 or later (macOS ships 3.2), curl, tar, and sha256sum or
# shasum.
set -euo pipefail

repo=turkosaurus/synth

fail() {
    echo "install.sh: $*" >&2
    exit 1
}

case "$(uname -s)/$(uname -m)" in
Linux/x86_64) target=x86_64-unknown-linux-gnu ;;
Darwin/arm64) target=aarch64-apple-darwin ;;
*) fail "no release is built for $(uname -s) $(uname -m)" ;;
esac
asset="synth-$target.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# HTTPS only, redirects included
get() {
    curl -fsSL --proto =https --proto-redir =https "$@"
}

if ! get -o "$tmp/release.json" "https://api.github.com/repos/$repo/releases/latest"; then
    fail "cannot read the latest release"
fi

# the API prints one key per line, and an asset's name comes before its digest
tag=$(sed -n 's/^  "tag_name": "\(.*\)",$/\1/p' "$tmp/release.json")
want=$(awk -v asset="$asset" '
    $0 ~ "\"name\": \"" asset "\"" { found = 1 }
    found && /"digest": "sha256:/ {
        sub(/.*"sha256:/, ""); sub(/".*/, ""); print; exit
    }
' "$tmp/release.json")
[ -n "$tag" ] || fail "the latest release has no tag"
[ -n "$want" ] || fail "$tag has no sha256 digest for $asset"

if ! get -o "$tmp/$asset" "https://github.com/$repo/releases/download/$tag/$asset"; then
    fail "cannot download $asset from $tag"
fi

if command -v sha256sum >/dev/null; then
    got=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
elif command -v shasum >/dev/null; then
    got=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
else
    fail "needs sha256sum or shasum to check the download"
fi
[ "$got" = "$want" ] || fail "$asset sha256 is $got, expected $want"

if ! tar -xzf "$tmp/$asset" -C "$tmp"; then
    fail "cannot unpack $asset"
fi
"$tmp/synth" install "$@"
