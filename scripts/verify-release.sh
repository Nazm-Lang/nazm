#!/usr/bin/env bash
# Verify a downloaded Nazm release (R1.5 D16): what users run, and what the release checklist runs
# after signing.
#
#   scripts/verify-release.sh DIR [IDENTITY ISSUER]
#
# DIR holds the downloaded archive(s), SHA256SUMS and SHA256SUMS.sigstore.json. IDENTITY and ISSUER
# are the signer the project publishes (SECURITY.md, nazm.dev); without them DIR/SIGNER is used,
# which is only as trustworthy as where DIR came from — pin the published values. Checks, in order:
# the Sigstore bundle verifies SHA256SUMS for exactly that identity and issuer (certificate,
# signature and transparency-log entry), then every archive present matches its line. Any failure
# exits non-zero: a tampered or unsigned download is refused, never warned about.
set -euo pipefail
dir="${1:?usage: scripts/verify-release.sh DIR [IDENTITY ISSUER]}"
command -v cosign >/dev/null || { echo "need cosign (brew install cosign)" >&2; exit 2; }
identity="${2:-}"; issuer="${3:-}"
if [ -z "$identity" ] && [ -f "$dir/SIGNER" ]; then
  identity=$(sed -n 's/^identity //p' "$dir/SIGNER"); issuer=$(sed -n 's/^issuer //p' "$dir/SIGNER")
fi
[ -n "$identity" ] && [ -n "$issuer" ] || { echo "no signer to check against: pass IDENTITY ISSUER" >&2; exit 2; }
[ -f "$dir/SHA256SUMS.sigstore.json" ] || { echo "unsigned: $dir/SHA256SUMS.sigstore.json is missing" >&2; exit 3; }

cosign verify-blob --bundle "$dir/SHA256SUMS.sigstore.json" \
  --certificate-identity "$identity" --certificate-oidc-issuer "$issuer" "$dir/SHA256SUMS" \
  || { echo "REFUSED: SHA256SUMS is not signed by $identity" >&2; exit 4; }

checked=0
while read -r digest name; do
  [ -f "$dir/$name" ] || continue
  got=$(shasum -a 256 "$dir/$name" | cut -d' ' -f1)
  [ "$got" = "$digest" ] || { echo "REFUSED: $name does not match its signed digest" >&2; exit 5; }
  checked=$((checked + 1))
done < "$dir/SHA256SUMS"
[ "$checked" -gt 0 ] || { echo "no listed file is present in $dir" >&2; exit 5; }
echo "verified: $checked file(s) signed by $identity"
