#!/usr/bin/env bash
# Sign a release with Sigstore (R1.5 D16): the copyright holder's own step at release time.
#
#   scripts/sign-release.sh DIR
#
# DIR holds what is published: the archives and SHA256SUMS (their digests). One signature covers
# them all: SHA256SUMS is signed, keylessly, with `cosign sign-blob`, which opens a browser for the
# signer to log in (GitHub); Sigstore's certificate authority binds that identity to a short-lived
# key, and the signature is recorded in the public transparency log (Rekor). Nothing secret is
# stored anywhere — there is no key file to keep or to leak. Writes DIR/SHA256SUMS.sigstore.json and
# records the signer's identity and issuer in DIR/SIGNER, which `scripts/verify-release.sh` and the
# published instructions check against.
#
# Run by the copyright holder only; never by an agent or CI, and never on unpublished work.
set -euo pipefail
dir="${1:?usage: scripts/sign-release.sh DIR}"
command -v cosign >/dev/null || { echo "need cosign (brew install cosign)" >&2; exit 2; }
[ -f "$dir/SHA256SUMS" ] || { echo "$dir/SHA256SUMS is missing: write the digests first" >&2; exit 2; }

# Every archive in DIR is listed in SHA256SUMS, and every listed file is there with that digest,
# before anything is signed: a signature over a stale list would vouch for the wrong files.
(cd "$dir" && shasum -a 256 -c SHA256SUMS) >/dev/null || { echo "SHA256SUMS does not match $dir" >&2; exit 3; }
for f in "$dir"/*.tar.gz "$dir"/*.zip; do
  [ -e "$f" ] || continue
  grep -q " $(basename "$f")\$" "$dir/SHA256SUMS" || { echo "$(basename "$f") is not in SHA256SUMS" >&2; exit 3; }
done

cosign sign-blob --yes --bundle "$dir/SHA256SUMS.sigstore.json" "$dir/SHA256SUMS"

# The identity the certificate names, for verifiers to pin.
python3 - "$dir/SHA256SUMS.sigstore.json" "$dir/SIGNER" <<'EOF'
import base64, json, re, subprocess, sys
bundle = json.load(open(sys.argv[1]))
vm = bundle.get("verificationMaterial", {})
der = (vm.get("certificate") or {}).get("rawBytes") or (vm.get("x509CertificateChain", {}).get("certificates", [{}])[0].get("rawBytes"))
pem = "-----BEGIN CERTIFICATE-----\n" + der + "\n-----END CERTIFICATE-----\n"
text = subprocess.run(["openssl", "x509", "-noout", "-text"], input=pem.encode(), capture_output=True).stdout.decode()
san = re.search(r"Subject Alternative Name:.*?\n\s*(?:email:|URI:)([^\s,]+)", text, re.S)
iss = re.search(r"1\.3\.6\.1\.4\.1\.57264\.1\.1:\s*\n\s*([^\s]+)", text)
open(sys.argv[2], "w").write("identity %s\nissuer %s\n" % (san.group(1) if san else "?", iss.group(1) if iss else "?"))
EOF
echo "signed: $dir/SHA256SUMS.sigstore.json"
cat "$dir/SIGNER"
