#!/bin/sh
# Ini source, in order: $KNXD_INI (content) > bind-mounted $KNXD_INI_FILE (default /etc/knxd/knxd.ini).
# ${VAR} references in the ini are expanded from the environment (envsubst), so host/port
# can be given at `docker run` time (-e KNX_HOST=... -e KNX_PORT=...).
# Any container args replace the whole command: `docker run knxrs-knxd:dev knxd --help`.
set -eu
if [ "$#" -gt 0 ]; then exec "$@"; fi

OUT=/tmp/knxd.ini
if [ -n "${KNXD_INI:-}" ]; then
  printf '%s\n' "$KNXD_INI" | envsubst > "$OUT"
elif [ -f "${KNXD_INI_FILE:-/etc/knxd/knxd.ini}" ]; then
  envsubst < "${KNXD_INI_FILE:-/etc/knxd/knxd.ini}" > "$OUT"
else
  echo "entrypoint: no ini (set KNXD_INI or mount ${KNXD_INI_FILE:-/etc/knxd/knxd.ini})" >&2
  exit 64
fi
echo "entrypoint: effective ini ($OUT):" >&2
cat "$OUT" >&2
exec knxd "$OUT"
