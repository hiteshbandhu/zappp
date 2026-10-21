#!/usr/bin/env bash
# One-time setup: creates a local code-signing certificate ("zappp Development")
# in your login keychain so every build of zappp has the same identity.
# macOS keys Microphone / Input Monitoring / Accessibility grants to that
# identity, so you stop re-granting them after each rebuild.
#
# The certificate must be trusted system-wide (admin domain), not just for
# your user: Accessibility and Input Monitoring are checked by the root TCC
# daemon, which ignores per-user trust and would otherwise log "Failed to
# match existing code requirement" and deny zappp even with the switch on.
#
# Run it in the Terminal app: sudo asks for your Mac password to trust the
# certificate system-wide. Safe to re-run.
# Undo with: security delete-identity -c "zappp Development"
set -euo pipefail
NAME="zappp Development"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if security find-certificate -c "$NAME" >/dev/null 2>&1; then
  echo "\"$NAME\" already exists."
  security find-certificate -c "$NAME" -p > "$tmp/cert.pem"
else
  cat > "$tmp/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" -config "$tmp/cert.cnf" 2>/dev/null
  openssl pkcs12 -export -legacy -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
    -out "$tmp/id.p12" -passout pass:zappp -name "$NAME" 2>/dev/null \
    || openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" \
         -out "$tmp/id.p12" -passout pass:zappp -name "$NAME"
  security import "$tmp/id.p12" -k ~/Library/Keychains/login.keychain-db -P zappp -T /usr/bin/codesign
  security add-trusted-cert -p codeSign -k ~/Library/Keychains/login.keychain-db "$tmp/cert.pem"
  echo "Created \"$NAME\"."
fi

if security dump-trust-settings -d 2>/dev/null | grep -q "$NAME"; then
  echo "Already trusted system-wide."
else
  echo "Trusting \"$NAME\" for code signing system-wide (enter your Mac password)…"
  # Writing the system keychain and admin trust settings needs root.
  sudo security add-trusted-cert -d -r trustRoot -p codeSign \
    -k /Library/Keychains/System.keychain "$tmp/cert.pem"
  echo "Done."
fi
echo "Now quit and reopen zappp so it re-checks its permissions."
