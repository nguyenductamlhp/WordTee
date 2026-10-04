#!/bin/sh
# Writes the htpasswd file nginx.conf reads (`auth_basic_user_file`) from
# BASIC_AUTH_USER and BASIC_AUTH_PASSWORD.
#
# The nginx image runs everything in /docker-entrypoint.d/ before nginx, and
# gives up if one of them fails, so a missing login keeps the site down rather
# than serving it open. Only the hash is written, into /tmp: the one writable
# path in the container.
set -eu

HTPASSWD=/tmp/htpasswd

fail() { echo "$0: error: $*" >&2; exit 1; }

[ -n "${BASIC_AUTH_USER:-}" ] || fail "BASIC_AUTH_USER is not set"
[ -n "${BASIC_AUTH_PASSWORD:-}" ] || fail "BASIC_AUTH_PASSWORD is not set"

# One `user:hash` per line, so the name can hold neither.
case "$BASIC_AUTH_USER" in
    *:* | *"
"*) fail "BASIC_AUTH_USER must not contain ':' or a line break" ;;
esac

# The password goes in on stdin (`-P 0`), keeping it off the process list.
hash="$(printf '%s' "$BASIC_AUTH_PASSWORD" | busybox mkpasswd -m sha512 -P 0)"

umask 077
printf '%s:%s\n' "$BASIC_AUTH_USER" "$hash" >"$HTPASSWD"
echo "$0: login enabled for user '$BASIC_AUTH_USER'"
