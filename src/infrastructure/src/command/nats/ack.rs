//! Ack-disiplinen kommandolytterne deler (SKU-0017 R8).
//!
//! Uten `max_deliver` og uten DLQ er en terminal sti i koden eneste vei ut for
//! en melding som aldri kan lykkes. Retry er ellers ubegrenset, med
//! eskalerende forsinkelse og eskalerende loggnivå — loggen er det eneste
//! stedet en evig retry blir synlig.

use std::time::Duration;

use async_nats::jetstream::{self, AckKind, message::Acker};
use tracing::{debug, error, warn};
use uuid::Uuid;

/// Antall leveringer serveren har gjort av denne meldingen. Første levering
/// er `1`.
///
/// `info()` leser reply-subjectet, som forsvinner i `split()`. Les den før du
/// splitter.
pub fn leveringsnummer(message: &jetstream::Message) -> i64 {
    message.info().map(|info| info.delivered).unwrap_or(1)
}

/// Meldingen er ferdig behandlet, uansett utfall. Ingen ny levering.
pub async fn ack_terminal(acker: &Acker) -> anyhow::Result<()> {
    acker
        .ack()
        .await
        .map_err(|err| anyhow::anyhow!("ack failed: {err}"))
}

/// Ny levering med forsinkelse som vokser med antall forsøk.
///
/// `Nak(None)` ville gitt umiddelbar redelivery. Et rotert Sikri-passord er
/// recoverable (SKU-0017 R2), så uten forsinkelse blir retryen en varm løkke
/// mot arkivet.
pub async fn nak_med_backoff(acker: &Acker, delivered: i64) -> anyhow::Result<()> {
    acker
        .ack_with(AckKind::Nak(Some(match delivered {
            ..=3 => Duration::from_secs(5),
            4..=10 => Duration::from_secs(30),
            _ => Duration::from_secs(300),
        })))
        .await
        .map_err(|err| anyhow::anyhow!("nak failed: {err}"))
}

/// Loggnivået følger hvor lenge meldingen har sirkulert.
///
/// De første forsøkene er normal drift. Ti er verdt et blikk. Hundre betyr at
/// noe krever inngripen, og skal se sånn ut i loggen.
pub fn logg_ny_levering(delivered: i64, command_id: Option<Uuid>, arsak: &str) {
    let command_id = command_id.map(|id| id.to_string());
    let command_id = command_id.as_deref();

    match delivered {
        ..=3 => debug!(delivered, command_id, arsak, "ber om ny levering"),
        4..=99 => warn!(
            delivered,
            command_id, arsak, "melding leveres fortsatt om igjen"
        ),
        _ => error!(
            delivered,
            command_id, arsak, "melding har feilet i over hundre leveringer"
        ),
    }
}
