#!/bin/sh
set -eu
# Only this disposable container trusts the fixture CA. No TLS-verification bypasses.
cp /public-ca/root.crt /usr/local/share/ca-certificates/fluxkit-e2e.crt
update-ca-certificates >/dev/null
mkdir -p "$HOME/.pki/nssdb" /results
certutil -N --empty-password -d "sql:$HOME/.pki/nssdb"
certutil -A -n fluxkit-e2e -t 'C,,' -i /public-ca/root.crt -d "sql:$HOME/.pki/nssdb"
export NODE_EXTRA_CA_CERTS=/public-ca/root.crt
exec node /work/run.mjs
