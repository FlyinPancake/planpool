#!/bin/sh
set -e

# When started as root, drop to the planpool user before exec'ing the server.
# Optional PUID/PGID remap the planpool user first (and chown /data to match).
# When started with --user, none of this applies — just run the binary.
if [ "$(id -u)" = "0" ]; then
    if [ -n "$PUID" ] || [ -n "$PGID" ]; then
        PUID="${PUID:-$(id -u planpool)}"
        PGID="${PGID:-$(id -g planpool)}"
        sed -i "s/^planpool:x:[0-9]*:[0-9]*:/planpool:x:${PUID}:${PGID}:/" /etc/passwd
        sed -i "s/^planpool:x:[0-9]*:/planpool:x:${PGID}:/" /etc/group
        chown -R "${PUID}:${PGID}" /data
    fi
    exec su-exec planpool:planpool /usr/local/bin/planpool "$@"
fi

exec /usr/local/bin/planpool "$@"
