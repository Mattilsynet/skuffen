#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/src/infrastructure/docker-compose.yaml"
MIGRATIONS_DIR="$ROOT_DIR/src/infrastructure/migrations"

DATABASE_USER="${DATABASE_USER:-skuffen}"
DATABASE_PASSWORD="${DATABASE_PASSWORD:-skuffen}"
DATABASE_NAME="${DATABASE_NAME:-skuffen}"
DATABASE_HOST="${DATABASE_HOST:-127.0.0.1}"
DATABASE_PORT="${DATABASE_PORT:-5433}"
DATABASE_URL="${DATABASE_URL:-postgres://${DATABASE_USER}:${DATABASE_PASSWORD}@${DATABASE_HOST}:${DATABASE_PORT}/${DATABASE_NAME}}"

NATS_PORT="${NATS_PORT:-4222}"
NATS_MONITOR_PORT="${NATS_MONITOR_PORT:-8222}"
NATS_URL="${NATS_URL:-nats://127.0.0.1:${NATS_PORT}}"

SKUFFEN_HOST="${SKUFFEN_HOST:-127.0.0.1}"
SKUFFEN_PORT="${SKUFFEN_PORT:-3001}"

SIKRI_SAKSNR="${SIKRI_SAKSNR:-2026/500983}"

KEEP_DB="${KEEP_DB:-0}"
KEEP_RUNNING="${KEEP_RUNNING:-0}"

DB_CONTAINER="local_skuffen_postgres"
NATS_CONTAINER=""
NATS_PID=""
SKUFFEN_PID=""
TMP_DIR=""

log() {
  printf '%s\n' "$*"
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    log "Missing required command: $1"
    exit 1
  fi
}

make_uuid() {
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen
  elif command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY'
import uuid
print(uuid.uuid4())
PY
  elif command -v python >/dev/null 2>&1; then
    python - <<'PY'
import uuid
print(uuid.uuid4())
PY
  else
    log "Missing uuid generator (uuidgen/python)."
    exit 1
  fi
}

compose_cmd() {
  if docker compose version >/dev/null 2>&1; then
    printf 'docker compose'
  elif command -v docker-compose >/dev/null 2>&1; then
    printf 'docker-compose'
  else
    log "Neither docker compose nor docker-compose is available."
    exit 1
  fi
}

cleanup() {
  if [[ "$KEEP_RUNNING" == "1" ]]; then
    log "KEEP_RUNNING=1 set, skipping cleanup."
    return
  fi

  if [[ -n "$SKUFFEN_PID" ]]; then
    kill "$SKUFFEN_PID" >/dev/null 2>&1 || true
  fi

  if [[ -n "$NATS_PID" ]]; then
    kill "$NATS_PID" >/dev/null 2>&1 || true
  fi

  if [[ -n "$NATS_CONTAINER" ]]; then
    docker rm -f "$NATS_CONTAINER" >/dev/null 2>&1 || true
  fi

  if [[ "$KEEP_DB" != "1" ]]; then
    local cmd
    cmd="$(compose_cmd)"
    DATABASE_USER="$DATABASE_USER" DATABASE_PASSWORD="$DATABASE_PASSWORD" DATABASE_NAME="$DATABASE_NAME" \
      $cmd -f "$COMPOSE_FILE" down -v >/dev/null 2>&1 || true
  fi

  if [[ -n "$TMP_DIR" && -d "$TMP_DIR" ]]; then
    rm -rf "$TMP_DIR"
  fi
}

trap cleanup EXIT

require_cmd docker
require_cmd cargo
require_cmd nats
require_cmd grep

if [[ -z "${BASE_URL_SIKRI:-}" ]]; then
  log "BASE_URL_SIKRI must be set to the Sikri base URL."
  exit 1
fi

if [[ -z "${APP_APPLICATION__PROJECT_ID:-}" ]]; then
  log "APP_APPLICATION__PROJECT_ID must be set (GCP project id for secrets)."
  exit 1
fi

TMP_DIR="$(mktemp -d)"
SKUFFEN_LOG="$TMP_DIR/skuffen.log"
NATS_LOG="$TMP_DIR/nats.log"

log "Starting Postgres (docker compose)"
COMPOSE_CMD="$(compose_cmd)"
DATABASE_USER="$DATABASE_USER" DATABASE_PASSWORD="$DATABASE_PASSWORD" DATABASE_NAME="$DATABASE_NAME" \
  $COMPOSE_CMD -f "$COMPOSE_FILE" up -d db

log "Waiting for Postgres to be ready"
for _ in {1..30}; do
  if docker exec "$DB_CONTAINER" pg_isready -U "$DATABASE_USER" -d "$DATABASE_NAME" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! docker exec "$DB_CONTAINER" pg_isready -U "$DATABASE_USER" -d "$DATABASE_NAME" >/dev/null 2>&1; then
  log "Postgres did not become ready."
  exit 1
fi

log "Applying migrations"
for file in "$MIGRATIONS_DIR"/*.up.sql; do
  docker exec -i "$DB_CONTAINER" psql -U "$DATABASE_USER" -d "$DATABASE_NAME" < "$file"
done

SKUFFEN_ID="$(make_uuid)"
CLIENT_REFERENCE="$(make_uuid)"
COMMAND_ID="$(make_uuid)"
CORRELATION_ID="$(make_uuid)"
COMMAND_ID_OPPRETT_SAK="$(make_uuid)"
COMMAND_ID_JOURNALPOST="$(make_uuid)"
COMMAND_ID_AVSLUTT_SAK="$(make_uuid)"
SAK_CLIENT_REFERENCE="$(make_uuid)"
JOURNALPOST_CLIENT_REFERENCE="$(make_uuid)"
DOKUMENT_CLIENT_REFERENCE="$(make_uuid)"
DOKUMENT_REFERANSE="$(make_uuid)"

log "Seeding id_mapping with sak $SIKRI_SAKSNR"
docker exec -i "$DB_CONTAINER" psql -U "$DATABASE_USER" -d "$DATABASE_NAME" \
  -v skuffen_id="$SKUFFEN_ID" \
  -v client_reference="$CLIENT_REFERENCE" \
  -v arkiv_id="$SIKRI_SAKSNR" \
  -v command_id="$COMMAND_ID" <<'SQL'
DELETE FROM id_mapping WHERE entity_type = 'sak' AND arkiv_id = :'arkiv_id';
INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id, command_id)
VALUES (:'skuffen_id'::uuid, 'sak', :'client_reference'::uuid, :'arkiv_id', :'command_id'::uuid);
SQL

log "Starting local NATS"
if command -v nats-server >/dev/null 2>&1; then
  nats-server -js -p "$NATS_PORT" -m "$NATS_MONITOR_PORT" > "$NATS_LOG" 2>&1 &
  NATS_PID="$!"
else
  NATS_CONTAINER="skuffen-local-nats"
  docker run -d --name "$NATS_CONTAINER" -p "$NATS_PORT":4222 -p "$NATS_MONITOR_PORT":8222 nats:2.10.7 \
    -js -p 4222 -m 8222 >/dev/null
fi

log "Starting skuffen"
export NATS_URL
export APP_ENV="local"
export APP_APPLICATION__HOST="$SKUFFEN_HOST"
export APP_APPLICATION__PORT="$SKUFFEN_PORT"
export DATABASE_USER
export DATABASE_PASSWORD
export DATABASE_NAME
export DATABASE_HOST
export DATABASE_PORT
export DATABASE_URL

cargo run --bin skuffen > "$SKUFFEN_LOG" 2>&1 &
SKUFFEN_PID="$!"

if command -v curl >/dev/null 2>&1; then
  log "Waiting for skuffen health check"
  for _ in {1..30}; do
    if curl -sSf "http://${SKUFFEN_HOST}:${SKUFFEN_PORT}/" >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
fi

REQUEST_PAYLOAD=$(printf '{"key":{"type":"arkivId","value":"%s"},"inkluderJournalposter":true}' "$SIKRI_SAKSNR")

log "Sending NATS request to sak.hent"
set +e
NATS_RESPONSE=$(nats --server "$NATS_URL" request sak.hent "$REQUEST_PAYLOAD")
NATS_STATUS=$?
set -e

printf '%s\n' "$NATS_RESPONSE"

if [[ $NATS_STATUS -ne 0 ]]; then
  log "NATS request failed with exit code $NATS_STATUS"
  exit $NATS_STATUS
fi

if printf '%s' "$NATS_RESPONSE" | grep -q '"status":"Error"'; then
  log "Sikri request returned Error."
  log "Check SIKRI_SAKSNR, BASE_URL_SIKRI, and secret access."
  exit 1
fi

DOKUMENT_FILE="$TMP_DIR/vedlegg.txt"
printf '%s\n' "Skuffen testvedlegg" > "$DOKUMENT_FILE"

log "Uploading media to arkiv_media"
nats --server "$NATS_URL" object put arkiv_media "$DOKUMENT_REFERANSE" "$DOKUMENT_FILE"

COMMAND_SEQUENCE_PAYLOAD=$(cat <<JSON
[
  {
    "command_id": "${COMMAND_ID_OPPRETT_SAK}",
    "correlation_id": "${CORRELATION_ID}",
    "payload": {
      "OpprettSak": {
        "client_reference": "${SAK_CLIENT_REFERENCE}",
        "sakstittel": "Skuffen E2E test",
        "arkivdel": "Tilsynsdivisjonene",
        "saksbehandler_id": "Z12345",
        "saksbehandler_enhet": "42",
        "ordningsverdi": "123"
      }
    }
  },
  {
    "command_id": "${COMMAND_ID_JOURNALPOST}",
    "correlation_id": "${CORRELATION_ID}",
    "payload": {
      "OpprettInterntNotatJournalpost": {
        "client_reference": "${JOURNALPOST_CLIENT_REFERENCE}",
        "tittel": "Internt notat",
        "dokument_dato": "2025-01-01",
        "saksbehandler": "Z12345",
        "saksbehandler_enhet": "42",
        "dokumenter": [
          {
            "client_reference": "${DOKUMENT_CLIENT_REFERENCE}",
            "tittel": "Vedlegg",
            "filtype": "PDF",
            "dokument_referanse": "${DOKUMENT_REFERANSE}"
          }
        ],
        "sak_key": {
          "type": "clientReference",
          "value": "${SAK_CLIENT_REFERENCE}"
        }
      }
    }
  },
  {
    "command_id": "${COMMAND_ID_AVSLUTT_SAK}",
    "correlation_id": "${CORRELATION_ID}",
    "payload": {
      "AvsluttSak": {
        "sak_key": {
          "type": "clientReference",
          "value": "${SAK_CLIENT_REFERENCE}"
        }
      }
    }
  }
]
JSON
)

log "Sending NATS request to arkiv.arkiver"
set +e
COMMAND_RESPONSE=$(nats --server "$NATS_URL" request arkiv.arkiver "$COMMAND_SEQUENCE_PAYLOAD")
COMMAND_STATUS=$?
set -e

printf '%s\n' "$COMMAND_RESPONSE"

if [[ $COMMAND_STATUS -ne 0 ]]; then
  log "NATS request failed with exit code $COMMAND_STATUS"
  exit $COMMAND_STATUS
fi

if printf '%s' "$COMMAND_RESPONSE" | grep -q '"status":"Error"'; then
  log "Command sequence returned Error."
  exit 1
fi

log "Logs saved to $TMP_DIR"
