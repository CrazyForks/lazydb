#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

cargo build --quiet --bin lazydb

home="$TMP/home"
config="$home/.config/lazydb"
bin="$home/.local/bin"
mkdir -p "$config/releases/0.1.0" "$bin"
printf '#!/bin/sh\nprintf %s\\n installed\n' 'ok' > "$config/releases/0.1.0/lazydb"
chmod 755 "$config/releases/0.1.0/lazydb"
ln -s "$config/releases/0.1.0" "$config/current"
ln -s "$config/current/lazydb" "$bin/lazydb"
printf '# user config\n# >>> LazyDB installer >>>\nexport PATH="$bin:\$PATH"\n# <<< LazyDB installer <<<\n' > "$home/.bashrc"
printf 'keep\n' > "$config/connections.toml"
printf 'keep\n' > "$config/unknown.db"
block_hash=$(python3 - "$home/.bashrc" <<'PY'
import hashlib, sys
text = open(sys.argv[1], encoding='utf-8').read()
begin = text.index('# >>> LazyDB installer >>>')
end = text.index('# <<< LazyDB installer <<<') + len('# <<< LazyDB installer <<<\n')
print(hashlib.sha256(text[begin:end].encode()).hexdigest())
PY
)
cat > "$config/install.json" <<EOF
{"schema":1,"product":"lazydb","manager":"native","channel":"stable","version":"0.1.0","target":"x86_64-unknown-linux-gnu","path":"$bin/lazydb","shell_profiles":[{"path":"$home/.bashrc","block_sha256":"$block_hash"}]}
EOF

before=$(find "$home" -type f -o -type l | sort)
HOME="$home" PATH="$bin:$PATH" "$ROOT/target/debug/lazydb" uninstall --dry-run --json > "$TMP/plan.json"
after=$(find "$home" -type f -o -type l | sort)
[ "$before" = "$after" ]
python3 - "$TMP/plan.json" <<'PY'
import json, sys
report = json.load(open(sys.argv[1], encoding='utf-8'))
assert report['status'] == 'planned'
assert any(item['kind'] == 'launcher' for item in report['actions'])
PY

HOME="$home" PATH="$bin:$PATH" "$ROOT/target/debug/lazydb" uninstall --yes >/dev/null
[ ! -e "$bin/lazydb" ]
[ ! -e "$config/current" ]
[ ! -e "$config/releases/0.1.0" ]
[ ! -e "$config/install.json" ]
[ -e "$config/connections.toml" ]
[ -e "$config/unknown.db" ]
[ "$(grep -c '# >>> LazyDB installer >>>' "$home/.bashrc")" -eq 0 ]
[ "$(grep -c '# user config' "$home/.bashrc")" -eq 1 ]
printf '%s\n' 'uninstaller tests: ok'
