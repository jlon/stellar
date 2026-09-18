#!/bin/sh
set -eu

# MinIO-style default: one data directory holds the database, the generated
# .jwt-secret and logs. `docker run IMAGE --help` and `--version` still work.
case "${1:-}" in
    "") set -- server /data ;;
esac

# Bind mounts may arrive with foreign ownership (uid mismatch between host and
# container). Normalize foreign-owned files (reused volumes), then drop to the
# unprivileged user before exec — the same pattern as the official postgres image.
find /data \! -user starrocks -exec chown starrocks:starrocks {} +
exec setpriv --reuid=starrocks --regid=starrocks --clear-groups /app/bin/stellar "$@"
