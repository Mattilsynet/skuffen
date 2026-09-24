//! Skriver dekomponeringsplanen: entitet, state og attributter.
//!
//! Hele planen går i samme transaksjon som operasjonsradene (SKU-0016 R12), så
//! en delvis skriving ikke kan overleve en crash.

use anyhow::{Context, Result};
use application::command::materialisering::{
    Dekomponeringsplan, DokumentRad, Dokumentkilde, JournalpostRad, Korrespondanseparter, SakRad,
};
use application::command::model::{Arkivdel, Korrespondansepart, Parttype, Utsendingsmottaker};
use domain::eksekvering::tilstand::JournalpostType;
use serde::Serialize;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

// DTO-er for JSONB-serialisering. Bor her fordi serde ikke skal inn i
// application (repo_rules).
#[derive(Serialize)]
#[serde(tag = "rolle", rename_all = "snake_case")]
enum KorrespondansepartJson {
    Avsender(PartJson),
    Mottaker(PartJson),
    Utsendingsmottaker(UtsendingsmottakerJson),
}

#[derive(Serialize)]
struct PartJson {
    navn: String,
    parttype: &'static str,
}

#[derive(Serialize)]
struct UtsendingsmottakerJson {
    navn: String,
    id_type: &'static str,
    id: String,
    adresse: String,
    postnummer: String,
    poststed: String,
}

fn parttype_kode(parttype: Parttype) -> &'static str {
    match parttype {
        Parttype::Person => "person",
        Parttype::Virksomhet => "virksomhet",
    }
}

fn part_json(part: &Korrespondansepart) -> PartJson {
    PartJson {
        navn: part.navn.clone(),
        parttype: parttype_kode(part.parttype),
    }
}

fn korrespondanseparter_json(parter: &Korrespondanseparter) -> serde_json::Value {
    let liste: Vec<KorrespondansepartJson> = match parter {
        Korrespondanseparter::Ingen => Vec::new(),
        Korrespondanseparter::Avsender(part) => {
            vec![KorrespondansepartJson::Avsender(part_json(part))]
        }
        Korrespondanseparter::Mottakere(parter) => parter
            .iter()
            .map(|part| KorrespondansepartJson::Mottaker(part_json(part)))
            .collect(),
        Korrespondanseparter::Utsendingsmottakere(mottakere) => mottakere
            .iter()
            .map(|mottaker| {
                KorrespondansepartJson::Utsendingsmottaker(utsendingsmottaker_json(mottaker))
            })
            .collect(),
    };
    serde_json::to_value(liste).expect("korrespondanseparter er serialiserbare")
}

fn utsendingsmottaker_json(mottaker: &Utsendingsmottaker) -> UtsendingsmottakerJson {
    use application::command::model::MottakerId;
    let (id_type, id) = match &mottaker.id {
        MottakerId::Person { fødselsnummer } => {
            ("fodselsnummer", fødselsnummer.as_str().to_string())
        }
        MottakerId::Virksomhet {
            organisasjonsnummer,
        } => (
            "organisasjonsnummer",
            organisasjonsnummer.as_str().to_string(),
        ),
    };
    UtsendingsmottakerJson {
        navn: mottaker.navn.clone(),
        id_type,
        id,
        adresse: mottaker.adresse.adresse.clone(),
        postnummer: mottaker.adresse.postnummer.as_str().to_string(),
        poststed: mottaker.adresse.poststed.clone(),
    }
}

fn arkivdel_kode(arkivdel: Arkivdel) -> &'static str {
    match arkivdel {
        Arkivdel::Tilsynsdivisjonene => "tilsynsdivisjonene",
        Arkivdel::Hovedkontoret => "hovedkontoret",
    }
}

fn journalposttype_kode(journalposttype: JournalpostType) -> &'static str {
    journalposttype.as_arkivkode()
}

pub(crate) async fn skriv_plan(
    tx: &mut Transaction<'_, Postgres>,
    plan: &Dekomponeringsplan,
) -> Result<()> {
    skriv_sak(tx, plan.command_id, &plan.sak).await?;
    if let Some(journalpost) = &plan.journalpost {
        skriv_journalpost(tx, plan.command_id, plan.sak.sak_id.into(), journalpost).await?;
    }
    for dokument in &plan.dokumenter {
        skriv_dokument(tx, plan.command_id, dokument).await?;
    }
    Ok(())
}

async fn sikre_entitet(
    tx: &mut Transaction<'_, Postgres>,
    skuffen_id: Uuid,
    entitet_type: &str,
    client_reference: Option<Uuid>,
    arkiv_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO entitet (skuffen_id, entitet_type, client_reference, arkiv_id)
           VALUES ($1, $2::entitet_type, $3, $4)
           ON CONFLICT (skuffen_id) DO NOTHING"#,
    )
    .bind(skuffen_id)
    .bind(entitet_type)
    .bind(client_reference)
    .bind(arkiv_id)
    .execute(&mut **tx)
    .await
    .context("failed to ensure entitet")?;
    Ok(())
}

async fn skriv_sak(
    tx: &mut Transaction<'_, Postgres>,
    command_id: Uuid,
    sak: &SakRad,
) -> Result<()> {
    let sak_id: Uuid = sak.sak_id.into();
    sikre_entitet(
        tx,
        sak_id,
        "sak",
        sak.client_reference,
        sak.arkiv_id.as_deref(),
    )
    .await?;

    let (sakstittel, arkivdel, ordningsverdi, saksbehandler_id, saksbehandler_enhet, tilgang) =
        match &sak.attributter {
            Some(a) => (
                Some(a.sakstittel.clone()),
                Some(arkivdel_kode(a.arkivdel)),
                Some(a.ordningsverdi.clone()),
                Some(a.saksbehandler_id.clone()),
                Some(a.saksbehandler_enhet.clone()),
                a.tilgang.clone(),
            ),
            None => (None, None, None, None, None, Default::default()),
        };

    // Attributter fra OpprettSak skal ikke overskrives av senere kommandoer
    // som treffer samme sak (SKU-0009 R5).
    sqlx::query(
        r#"INSERT INTO sak_tilstand (
               sak_id, tilstand, sakstittel, arkivdel, ordningsverdi,
               saksbehandler_id, saksbehandler_enhet, tilgangskode, tilgangshjemmel,
               oensket_saksansvarlig_id, oensket_saksansvarlig_enhet, opprettet_av_command_id
           )
           VALUES ($1, CASE WHEN $12 IS NULL THEN 'opprettet' ELSE 'ikke_opprettet' END,
                   $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
           ON CONFLICT (sak_id) DO UPDATE SET
               oensket_saksansvarlig_id = COALESCE(EXCLUDED.oensket_saksansvarlig_id,
                                                   sak_tilstand.oensket_saksansvarlig_id),
               oensket_saksansvarlig_enhet = COALESCE(EXCLUDED.oensket_saksansvarlig_enhet,
                                                      sak_tilstand.oensket_saksansvarlig_enhet)"#,
    )
    .bind(sak_id)
    .bind(sakstittel)
    .bind(arkivdel)
    .bind(ordningsverdi)
    .bind(saksbehandler_id)
    .bind(saksbehandler_enhet)
    .bind(tilgang.tilgangskode())
    .bind(tilgang.tilgangshjemmel())
    .bind(sak.oensket_saksansvarlig.as_ref().map(|(id, _)| id.clone()))
    .bind(
        sak.oensket_saksansvarlig
            .as_ref()
            .map(|(_, enhet)| enhet.clone()),
    )
    .bind(command_id)
    .bind(sak.attributter.as_ref().map(|_| 1i32))
    .execute(&mut **tx)
    .await
    .context("failed to write sak_tilstand")?;

    Ok(())
}

async fn skriv_journalpost(
    tx: &mut Transaction<'_, Postgres>,
    command_id: Uuid,
    sak_id: Uuid,
    journalpost: &JournalpostRad,
) -> Result<()> {
    let journalpost_id: Uuid = journalpost.journalpost_id.into();
    sikre_entitet(
        tx,
        journalpost_id,
        "journalpost",
        Some(journalpost.client_reference),
        None,
    )
    .await?;

    let a = &journalpost.attributter;
    sqlx::query(
        r#"INSERT INTO journalpost_tilstand (
               journalpost_id, sak_id, tilstand, journalposttype, med_utsending,
               tittel, dokument_dato, saksbehandler_id, saksbehandler_enhet,
               tilgangskode, tilgangshjemmel, korrespondanseparter, kildesystem,
               opprettet_av_command_id
           )
           VALUES ($1, $2, 'ikke_opprettet', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
           ON CONFLICT (journalpost_id) DO NOTHING"#,
    )
    .bind(journalpost_id)
    .bind(sak_id)
    .bind(journalposttype_kode(a.journalposttype))
    .bind(a.med_utsending)
    .bind(&a.tittel)
    .bind(&a.dokument_dato)
    .bind(&a.saksbehandler_id)
    .bind(&a.saksbehandler_enhet)
    .bind(a.tilgang.tilgangskode())
    .bind(a.tilgang.tilgangshjemmel())
    .bind(korrespondanseparter_json(&a.korrespondanseparter))
    .bind(a.kildesystem.clone())
    .bind(command_id)
    .execute(&mut **tx)
    .await
    .context("failed to write journalpost_tilstand")?;

    Ok(())
}

async fn skriv_dokument(
    tx: &mut Transaction<'_, Postgres>,
    command_id: Uuid,
    dokument: &DokumentRad,
) -> Result<()> {
    let dokument_id: Uuid = dokument.dokument_id.into();
    sikre_entitet(
        tx,
        dokument_id,
        "dokument",
        Some(dokument.client_reference),
        None,
    )
    .await?;

    let a = &dokument.attributter;
    // En mal må rendres først; bytes er klare med én gang.
    let (tilstand, filtype, dokument_referanse, mal_referanse, felter) = match &a.kilde {
        Dokumentkilde::Bytes {
            dokument_referanse,
            filtype,
        } => (
            "klar",
            Some(filtype.clone()),
            Some(*dokument_referanse),
            None,
            None,
        ),
        Dokumentkilde::HtmlTemplate {
            mal_referanse,
            felter,
            ..
        } => {
            let felter: Vec<&str> = felter.iter().map(|felt| felt.as_token()).collect();
            (
                "avventer_rendring",
                None,
                None,
                Some(*mal_referanse),
                Some(serde_json::to_value(felter).expect("felter er serialiserbare")),
            )
        }
    };

    sqlx::query(
        r#"INSERT INTO dokument_tilstand (
               dokument_id, journalpost_id, tilstand, rekkefolge, er_hoveddokument,
               tittel, filtype, dokument_referanse, mal_referanse, felter,
               opprettet_av_command_id
           )
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
           ON CONFLICT (dokument_id) DO NOTHING"#,
    )
    .bind(dokument_id)
    .bind(Uuid::from(dokument.journalpost_id))
    .bind(tilstand)
    .bind(i32::from(a.rekkefolge))
    .bind(a.er_hoveddokument())
    .bind(&a.tittel)
    .bind(filtype)
    .bind(dokument_referanse)
    .bind(mal_referanse)
    .bind(felter)
    .bind(command_id)
    .execute(&mut **tx)
    .await
    .context("failed to write dokument_tilstand")?;

    Ok(())
}
