#!/bin/sh
set -eu

PATH="$HOME/.local/bin:$HOME/.stado/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
export PATH
export WC_AGENT_SKARBIEC_URL="http://127.0.0.1:8895"
export WC_AGENT_SKARBIEC_CONSUMER="stado-local-agent"
export WC_AGENT_SKARBIEC_TOKEN_FILE="$HOME/.stado/local-agent-skarbiec-token"
export WC_AGENT_SKARBIEC_ITEMS="compute-marketplace-agent,jeden-model-router,jeden-agent-auth,probierz-model-router,trading-autonomy-agent-auth,trading-autonomy-media-router,trading-autonomy-model-router,wisent-backend-alert-router,wisent-backend-data-router,wisent-backend-inactivity-webhook,wisent-backend-media-router,wisent-backend-model-router,wisent-backend-object-client,wisent-backend-object-signing,wisent-backend-release-runner,wisent-backend-scheduler,wisent-trade-agent-email,wisent-trade-agent-model-router"
export WC_AGENT_SKARBIEC_SECRET_FIELDS="compute-marketplace-agent#token,jeden-model-router#token,jeden-agent-auth#agent_auth_secret,probierz-model-router#token,trading-autonomy-agent-auth#token,trading-autonomy-media-router#token,trading-autonomy-model-router#token,wisent-backend-alert-router#token,wisent-backend-data-router#token,wisent-backend-inactivity-webhook#secret,wisent-backend-media-router#token,wisent-backend-model-router#token,wisent-backend-object-client#token,wisent-backend-object-signing#key,wisent-backend-release-runner#token,wisent-backend-scheduler#token,wisent-trade-agent-email#token,wisent-trade-agent-model-router#token"

exec "$HOME/.stado/bin/stado" bootstrap --local --target charless-mac-mini
