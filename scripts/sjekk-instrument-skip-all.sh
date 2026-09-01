#!/usr/bin/env bash
# SKU-0019 R2: `#[instrument]` skal alltid ha `skip_all`.
#
# Uten `skip_all` registrerer makroen alle argumenter via `Debug`. I
# `application` er argumentene kommandopayloads med korrespondanseparter, og de
# ville havnet i `jsonPayload` på `info!`-nivå. Dette er den mekaniske
# garantien mot at det skjer ved et uhell.
set -euo pipefail

funn=$(find src crates -name '*.rs' -not -path '*/target/*' -print0 \
  | xargs -0 awk '
      # Attributtet kan gå over flere linjer. Samler fra åpning til `)]`.
      !samler && /^[[:space:]]*#\[(tracing::)?instrument/ {
        start = FNR
        buf = $0
        if (buf !~ /\(/) { print FILENAME ":" start ": " buf; next }
        samler = 1
      }
      samler && FNR > start { buf = buf " " $0 }
      samler && buf ~ /\)\]/ {
        if (buf !~ /skip_all/) print FILENAME ":" start ": " buf
        samler = 0
      }
    ')

if [[ -n "$funn" ]]; then
  echo "Fant #[instrument] uten skip_all (SKU-0019 R2):"
  echo "$funn"
  exit 1
fi

echo "Alle #[instrument]-attributter har skip_all."
