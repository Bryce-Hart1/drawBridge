#!/usr/bin/env bash
# Claude — Date 11/04/2026
# Acceptance checks for a drawBridge store as clients reach it: swiftSearch V2
# behind castle+, usually behind a Cloudflare Tunnel too. Everything goes
# through the public URL, so it checks what each layer must get right for
# drawBridge:
#   - swiftSearch's own keys reach it (castle+ has no `auth` on the route)
#   - only /v1/blobs/ and /healthz get through; admin and box routes don't
#   - uploads arrive with Content-Length (not chunked), up to 64 MiB, no more
#   - Range, ETag and If-None-Match pass through untouched
#   - nothing is served from a cache in between
#
#   H=https://vault.example.com RW=<write key> RO=<read key> scripts/check_vault.sh
#
# Keys can also come from a file of RW=… / RO=… lines: KEYS=path/to/test-keys.
# Writes only under check/<pid>/ and deletes it again.
set -u

: "${H:?set H to the store URL, e.g. https://vault.example.com}"
if [ -n "${KEYS:-}" ]; then
    # shellcheck disable=SC1090
    . "$KEYS"
fi
: "${RW:?set RW to a write key (or KEYS to a file holding one)}"
: "${RO:?set RO to a read key (or KEYS to a file holding one)}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
pass=0
fail=0

check() {  # name expected actual
    if [ "$2" = "$3" ]; then
        printf '  ok    %s\n' "$1"
        pass=$((pass + 1))
    else
        printf '  FAIL  %s (expected %s, got %s)\n' "$1" "$2" "$3"
        fail=$((fail + 1))
    fi
}

code() {  # curl args... -> status code only
    curl -s -o /dev/null -w '%{http_code}' "$@"
}

blocked() {  # curl args... -> "blocked" if the edge refused it before swiftSearch saw it
    local out status
    out=$(curl -s -w '\n%{http_code}' "$@")
    status=${out##*$'\n'}
    # swiftSearch's own answers are JSON with a "code"; castle's 404 isn't.
    if [ "$status" = 404 ] && ! printf '%s' "$out" | grep -q '"code":'; then echo blocked; else echo "$status"; fi
}

AW="Authorization: Bearer $RW"
AR="Authorization: Bearer $RO"
base="check/$$"
key="$base/chunk"

head -c 8388608 /dev/urandom > "$work/chunk"       # one drawBridge chunk
head -c 600000 /dev/urandom > "$work/600k.bin"
head -c 70000000 /dev/zero > "$work/big.bin"

echo "reachable"
check "/healthz without a key -> 200" 200 "$(code "$H/healthz")"

echo "keys"
check "no key -> 401" 401 "$(code "$H/v1/blobs/$key")"
check "unknown key -> 401" 401 "$(code -H 'Authorization: Bearer ssk_00000000_nope' "$H/v1/blobs/$key")"
unauth=$(curl -s "$H/v1/blobs/$key")
check "the 401 is swiftSearch's, not a gateway's" unauthorized \
    "$(printf '%s' "$unauth" | sed -n 's/.*"code":"\([a-z_]*\)".*/\1/p')"

echo "write key"
check "PUT one 8 MiB chunk -> 201" 201 \
    "$(code -X PUT -H "$AW" -H 'X-SwiftSearch-Type: bin' --data-binary @"$work/chunk" "$H/v1/blobs/$key")"
curl -s -H "$AW" -o "$work/back" "$H/v1/blobs/$key"
check "GET round-trips byte for byte" same "$(cmp -s "$work/chunk" "$work/back" && echo same || echo differ)"

hdrs=$(curl -s -H "$AW" -H 'Range: bytes=100-199' -D - -o "$work/range" "$H/v1/blobs/$key")
check "Range -> 206" 206 "$(printf '%s' "$hdrs" | head -1 | awk '{print $2}')"
check "Range -> Content-Range" 1 "$(printf '%s' "$hdrs" | grep -ci '^content-range: bytes 100-199/8388608')"
check "Range -> the right 100 bytes" same \
    "$(cmp -s <(dd if="$work/chunk" bs=1 skip=100 count=100 2>/dev/null) "$work/range" && echo same || echo differ)"

etag=$(curl -s -H "$AW" -I "$H/v1/blobs/$key" | awk 'tolower($1)=="etag:" {print $2}' | tr -d '\r')
check "If-None-Match -> 304" 304 "$(code -H "$AW" -H "If-None-Match: $etag" "$H/v1/blobs/$key")"

cache=$(curl -s -H "$AW" -D - -o /dev/null "$H/v1/blobs/$key" | awk 'tolower($1)=="cf-cache-status:" {print $2}' | tr -d '\r')
check "not served from Cloudflare's cache" no "$([ "$cache" = HIT ] && echo yes || echo no)"

echo "read key"
check "GET -> 200" 200 "$(code -H "$AR" "$H/v1/blobs/$key")"
check "PUT -> 403" 403 "$(code -X PUT -H "$AR" --data-binary @"$work/600k.bin" "$H/v1/blobs/$base/ro")"
check "DELETE -> 403" 403 "$(code -X DELETE -H "$AR" "$H/v1/blobs/$key")"

echo "edge"
check "/v1/admin/limits blocked at the edge" blocked "$(blocked -H "$AW" "$H/v1/admin/limits")"
check "/v1/box/ blocked at the edge" blocked "$(blocked -X POST -H "$AW" "$H/v1/box/$base-boxed?first=$key&second=$base/none")"
check "/v1/unbox/ blocked at the edge" blocked "$(blocked -X POST -H "$AW" "$H/v1/unbox/$base")"
check "/v1/prepare/ blocked at the edge" blocked "$(blocked -X POST -H "$AW" "$H/v1/prepare/$key")"
check "70 MB PUT -> 413" 413 "$(code -X PUT -H "$AW" --data-binary @"$work/big.bin" "$H/v1/blobs/$base/big")"

echo "cleanup"
check "DELETE -> 204" 204 "$(code -X DELETE -H "$AW" "$H/v1/blobs/$key")"
check "deleted -> 404" 404 "$(code -H "$AW" "$H/v1/blobs/$key")"

echo
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
