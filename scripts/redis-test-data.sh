#!/usr/bin/env bash
set -euo pipefail

REDIS_CLI=(redis-cli "${REDIS_URL:-}" )

# Keep the default local setup convenient while allowing callers to provide a
# URL such as redis://127.0.0.1:6379/2.
if [[ -z "${REDIS_URL:-}" ]]; then
  REDIS_CLI=(redis-cli -h "${REDIS_HOST:-127.0.0.1}" -p "${REDIS_PORT:-6379}" -n "${REDIS_DB:-0}")
else
  REDIS_CLI=(redis-cli -u "$REDIS_URL")
fi

"${REDIS_CLI[@]}" ping >/dev/null

# Remove only this fixture's namespace so the script is safe to re-run without
# touching unrelated keys in the selected Redis database.
"${REDIS_CLI[@]}" --scan --pattern 'lazydb:test:*' | while IFS= read -r key; do
  [[ -z "$key" ]] || "${REDIS_CLI[@]}" del "$key" >/dev/null
done

# String: normal UTF-8, valid JSON, YAML-like text, and binary-looking data.
"${REDIS_CLI[@]}" set lazydb:test:string "LazyDB Redis fixture" >/dev/null
"${REDIS_CLI[@]}" set lazydb:test:json '{"id":42,"name":"Ada Lovelace","active":true,"tags":["redis","lazydb"]}' >/dev/null
"${REDIS_CLI[@]}" set lazydb:test:yaml $'name: lazydb\nversion: 1\nfeatures:\n  - redis\n  - preview' >/dev/null
"${REDIS_CLI[@]}" setex lazydb:test:expiring 3600 "expires in one hour" >/dev/null

# Hash: good for the Redis Table projection.
"${REDIS_CLI[@]}" hset lazydb:test:hash \
  id 1001 \
  username ada \
  email ada@example.test \
  role maintainer \
  active true >/dev/null

# List: ordered values and enough items to exercise paging.
"${REDIS_CLI[@]}" rpush lazydb:test:list \
  "first item" "second item" "third item" "fourth item" "fifth item" >/dev/null

# Set: unordered unique members.
"${REDIS_CLI[@]}" sadd lazydb:test:set rust redis docker terminal >/dev/null

# Sorted set: scores are visible in the preview.
"${REDIS_CLI[@]}" zadd lazydb:test:zset \
  98.5 "Ada" \
  91.0 "Grace" \
  87.25 "Linus" \
  76.0 "Margaret" >/dev/null

# Stream: fields exercise stream IDs and derived field columns.
"${REDIS_CLI[@]}" xadd lazydb:test:stream '*' event connected user ada >/dev/null
"${REDIS_CLI[@]}" xadd lazydb:test:stream '*' event query user grace query "SCAN 0" >/dev/null
"${REDIS_CLI[@]}" xadd lazydb:test:stream '*' event mutation user linus key lazydb:test:string >/dev/null

echo "Redis test data loaded into database ${REDIS_DB:-0}."
"${REDIS_CLI[@]}" --scan --pattern 'lazydb:test:*' | sort
