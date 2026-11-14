#!/usr/bin/env bash
# Downloads the pinned sops release for one target into vendor/sops/ and
# verifies it: the checksums file with cosign, then the binary's SHA-256.
#
# Usage: scripts/fetch-sops.sh <rust-target-triple>
# Set SOPS_SKIP_COSIGN=1 to skip the signature check when cosign isn't
# installed (local use only; the SHA-256 check still runs).
set -euo pipefail

SOPS_VERSION="v3.13.3" # renovate: datasource=github-releases depName=getsops/sops

target="${1:?usage: fetch-sops.sh <rust-target-triple>}"
case "$target" in
  x86_64-unknown-linux-gnu)  asset="sops-${SOPS_VERSION}.linux.amd64";  name="sops" ;;
  aarch64-unknown-linux-gnu) asset="sops-${SOPS_VERSION}.linux.arm64";  name="sops" ;;
  x86_64-apple-darwin)       asset="sops-${SOPS_VERSION}.darwin.amd64"; name="sops" ;;
  aarch64-apple-darwin)      asset="sops-${SOPS_VERSION}.darwin.arm64"; name="sops" ;;
  x86_64-pc-windows-msvc)    asset="sops-${SOPS_VERSION}.amd64.exe";    name="sops.exe" ;;
  aarch64-pc-windows-msvc)   asset="sops-${SOPS_VERSION}.arm64.exe";    name="sops.exe" ;;
  *) echo "fetch-sops: unsupported target: $target" >&2; exit 1 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/vendor/sops"
base="https://github.com/getsops/sops/releases/download/${SOPS_VERSION}"
checksums="sops-${SOPS_VERSION}.checksums.txt"
bundle="sops-${SOPS_VERSION}.checksums.sigstore.json"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

fetch() { curl --fail --silent --show-error --location --retry 3 -o "$2" "$1"; }
fetch "$base/$asset" "$asset"
fetch "$base/$checksums" "$checksums"

if [ "${SOPS_SKIP_COSIGN:-}" = "1" ]; then
  echo "fetch-sops: skipping the cosign signature check (SOPS_SKIP_COSIGN=1)" >&2
else
  fetch "$base/$bundle" "$bundle"
  cosign verify-blob "$checksums" \
    --bundle "$bundle" \
    --certificate-identity-regexp=https://github.com/getsops \
    --certificate-oidc-issuer=https://token.actions.githubusercontent.com
fi

expected="$(awk -v f="$asset" '$2 == f || $2 == "*" f { print $1 }' "$checksums")"
if [ -z "$expected" ]; then
  echo "fetch-sops: $asset is not listed in $checksums" >&2; exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$asset" | awk '{ print $1 }')"
else
  actual="$(shasum -a 256 "$asset" | awk '{ print $1 }')"
fi
if [ "$expected" != "$actual" ]; then
  echo "fetch-sops: SHA-256 mismatch for $asset" >&2; exit 1
fi

# The MPL 2.0 requires shipping the licence text with the binary.
fetch "https://raw.githubusercontent.com/getsops/sops/${SOPS_VERSION}/LICENSE" sops-LICENSE.txt

rm -rf "$out"
mkdir -p "$out"
mv "$asset" "$out/$name"
mv sops-LICENSE.txt "$out/sops-LICENSE.txt"
chmod +x "$out/$name"
echo "fetch-sops: ${SOPS_VERSION} for $target is in vendor/sops/"
