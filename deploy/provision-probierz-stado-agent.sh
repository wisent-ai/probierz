#!/bin/sh
set -eu
umask u=rwx,g=,o=
PATH="$HOME/.local/bin:$HOME/.stado/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
export PATH

SKARBIEC_BIN=${SKARBIEC_BIN:-"$HOME/.stado/bin/skarbiec"}
SKARBIEC_VAULT_FILE=${SKARBIEC_VAULT_FILE:-"$HOME/.stado/skarbiec.vault.json"}
TOKEN_FILE=${TOKEN_FILE:-"$HOME/.stado/local-agent-skarbiec-token"}
CONSUMER=stado-local-agent
REQUIRED_CAPABILITIES="read:probierz-model-router#token read:probierz-agent-auth#agent_auth_secret"

items=$(mktemp)
auth_payload=$(mktemp)
listing=$(mktemp)
capabilities_file=$(mktemp)
minted=$(mktemp)
token_tmp=$(mktemp)
trap 'rm -f "$items" "$auth_payload" "$listing" "$capabilities_file" "$minted" "$token_tmp"' EXIT HUP INT TERM

SKARBIEC_VAULT_FILE="$SKARBIEC_VAULT_FILE" "$SKARBIEC_BIN" list > "$items"
if ! python3 - "$items" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    items = json.load(handle)
raise SystemExit(0 if any(item.get("id") == "probierz-agent-auth" and not item.get("deleted", False) for item in items) else 1)
PY
then
  python3 > "$auth_payload" <<'PY'
import json
import secrets

print(json.dumps({
    "schema": "skarbiec.item.v2",
    "kind": "internal-authority",
    "fields": {
        "id": "probierz",
        "agent_auth_secret": secrets.token_urlsafe(48),
    },
    "context": {},
}, separators=(",", ":")))
PY
  SKARBIEC_VAULT_FILE="$SKARBIEC_VAULT_FILE" "$SKARBIEC_BIN" set-json \
    probierz-agent-auth --type internal-authority < "$auth_payload"
fi
SKARBIEC_VAULT_FILE="$SKARBIEC_VAULT_FILE" "$SKARBIEC_BIN" tokens > "$listing"
python3 - "$listing" "$CONSUMER" $REQUIRED_CAPABILITIES > "$capabilities_file" <<'PY'
import json
import sys

listing_path, consumer, *required = sys.argv[1:]
with open(listing_path, encoding="utf-8") as handle:
    registrations = json.load(handle)
registration = next((entry for entry in registrations if entry.get("consumer") == consumer), None)
if registration is None:
    raise SystemExit(f"missing existing {consumer} grant")
encoded = []
for capability in registration.get("capabilities") or []:
    action = capability.get("action")
    item = capability.get("item")
    field = capability.get("field")
    if not action or not item:
        raise SystemExit(f"invalid capability in {consumer} grant")
    encoded.append(f"{action}:{item}" + (f"#{field}" if field else ""))
encoded.extend(required)
print(",".join(sorted(set(encoded))))
PY

capabilities=$(cat "$capabilities_file")
SKARBIEC_VAULT_FILE="$SKARBIEC_VAULT_FILE" "$SKARBIEC_BIN" token-mint "$CONSUMER" \
  --capabilities "$capabilities" \
  --replace-capabilities > "$minted"
python3 - "$minted" > "$token_tmp" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    token = json.load(handle).get("token")
if not isinstance(token, str) or not token or any(character.isspace() for character in token):
    raise SystemExit("token-mint returned an invalid bearer")
print(token)
PY
chmod 600 "$token_tmp"
mv "$token_tmp" "$TOKEN_FILE"
printf '{"consumer":"%s","capabilities":["read:probierz-model-router#token","read:probierz-agent-auth#agent_auth_secret"],"token_file":"%s"}\n' \
  "$CONSUMER" "$TOKEN_FILE"
