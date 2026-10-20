//! Kommandoansvar og admin-forkasting ende-til-ende (SKU-0024).
//!
//! En kommando avvist i validering holder saken åpen: `AvsluttSak` venter til
//! kommandoen er retryet til fullføring eller eksplisitt forkastet.

use std::time::Duration;

use anyhow::Result;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::AvsluttSak;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::status::SkuffenCommandEvent;
use lib_schemas::skuffen::tilgang::Tilgjengelighet;
use serde_json::{Value, json};
use uuid::Uuid;

use support::{
    CommandScenario, admin_forkast, admin_hent_command, admin_retry, publish_media,
    send_command_batch, wait_for_command_event,
};

mod support;

const VENT: Duration = Duration::from_secs(30);

fn assert_ack(svar: &Value) {
    assert_eq!(*svar, json!({ "status": "Ok", "payload": null }), "{svar}");
}

/// Venter til avslutningsoperasjonen er vurdert og blokkert.
async fn vent_pa_blokkert_avslutning(nats_url: &str, command_id: Uuid) -> Result<Value> {
    let frist = tokio::time::Instant::now() + VENT;
    loop {
        let svar = admin_hent_command(nats_url, command_id).await?;
        let operasjon = &svar["payload"]["operasjoner"][0];
        if operasjon["status"] == "blokkert" && !operasjon["siste_detalj"].is_null() {
            return Ok(operasjon.clone());
        }
        anyhow::ensure!(
            tokio::time::Instant::now() < frist,
            "avslutningen ble aldri blokkert: {svar}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avvist_kommando_holder_saken_aapen_til_den_forkastes() -> Result<()> {
    let env = support::start_runtime().await?;
    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let opprett_sak = scenario.opprett_sak_med_tilgjengelighet(
        "Z12345",
        "42",
        format!("Forkast {}", Uuid::new_v4()),
        Tilgjengelighet::Offentlig,
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    wait_for_command_event(
        &env.nats_url,
        opprett_sak.command_id,
        SkuffenCommandEvent::Fullfort,
        VENT,
    )
    .await?;

    let sak_key = DtoSakKey::ClientReference(scenario.sak_client_reference);
    let avvist = scenario.opprett_internt_notat_med_ugyldig_markup(
        "Z12345",
        "42",
        sak_key.clone(),
        "Notat med [skjermet] uten skjerming",
    );
    let avslutt = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak { sak_key }),
    };
    send_command_batch(&env.nats_url, &[avvist.clone(), avslutt.clone()]).await?;
    wait_for_command_event(
        &env.nats_url,
        avvist.command_id,
        SkuffenCommandEvent::Avvist,
        VENT,
    )
    .await?;

    let operasjon = vent_pa_blokkert_avslutning(&env.nats_url, avslutt.command_id).await?;
    assert_eq!(
        operasjon["siste_detalj"], "blocked_reason=sosken_ikke_ferdige",
        "saken lukkes ikke over en avvist kommando"
    );

    // Retry endrer ingenting når årsaken ikke er rettet.
    assert_ack(&admin_retry(&env.nats_url, &[avslutt.command_id]).await?);
    let operasjon = vent_pa_blokkert_avslutning(&env.nats_url, avslutt.command_id).await?;
    assert_eq!(operasjon["status"], "blokkert");

    assert_ack(&admin_forkast(&env.nats_url, &[avvist.command_id]).await?);
    wait_for_command_event(
        &env.nats_url,
        avslutt.command_id,
        SkuffenCommandEvent::Fullfort,
        VENT,
    )
    .await?;

    // Forkasting er endelig; klienten må sende en ny kommando.
    let svar = admin_retry(&env.nats_url, &[avvist.command_id]).await?;
    assert_eq!(svar["payload"]["message"], "Command is discarded", "{svar}");

    // Idempotent, og en gjenutsendelse gjenåpner ikke kommandoen.
    assert_ack(&admin_forkast(&env.nats_url, &[avvist.command_id]).await?);
    send_command_batch(&env.nats_url, std::slice::from_ref(&avvist)).await?;
    let command = admin_hent_command(&env.nats_url, avvist.command_id).await?;
    assert!(
        command["payload"]["operasjoner"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    Ok(())
}
