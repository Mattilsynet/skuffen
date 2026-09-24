//! Read-only projection av lokal reparasjonstilstand.
//!
//! Hvert oppslag kjører i én `REPEATABLE READ READ ONLY`-transaksjon, slik at
//! command og operasjoner — eller sak og barn — kommer fra samme snapshot selv
//! mens workeren oppdaterer operasjoner. Adapteren leser aldri
//! `operasjon_forsok`, `arkiv_command_inbox`, object store, status-streamen
//! eller arkivet, og den skriver ingenting.

use anyhow::{Context, Result, anyhow};
use application::admin::model::{
    AdminCommand, AdminDokument, AdminEntitetIdentitet, AdminJournalpost, AdminKorrespondansepart,
    AdminOperasjonDetaljer, AdminOperasjonEntitet, AdminOperasjonSammendrag, AdminSak,
    AdminSakFakta, AdminSakNokkel,
};
use application::admin::ports::admin_read_repository::AdminReadRepository;
use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::operasjon::{EntitetId, OperasjonId};
use lib_sql::database_config::DbPool;
use serde::Deserialize;
use sqlx::{Postgres, Row, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

const SNAPSHOT_BEGIN: &str = "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY";

pub struct PostgresAdminReadRepository {
    pool: DbPool,
}

impl PostgresAdminReadRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AdminReadRepository for PostgresAdminReadRepository {
    async fn hent_command(&self, command_id: Uuid) -> Result<Option<AdminCommand>> {
        let mut tx = self
            .pool
            .begin_with(SNAPSHOT_BEGIN)
            .await
            .context("failed to begin admin snapshot")?;
        let resultat = command_i_snapshot(&mut tx, command_id).await;
        avslutt(tx, resultat).await
    }

    async fn hent_sak(&self, key: AdminSakNokkel) -> Result<Option<AdminSak>> {
        let mut tx = self
            .pool
            .begin_with(SNAPSHOT_BEGIN)
            .await
            .context("failed to begin admin snapshot")?;
        let resultat = sak_i_snapshot(&mut tx, key).await;
        avslutt(tx, resultat).await
    }
}

/// Både `Some`-, `None`- og feilveien avsluttes eksplisitt.
async fn avslutt<T>(tx: Transaction<'_, Postgres>, resultat: Result<T>) -> Result<T> {
    match resultat {
        Ok(verdi) => {
            tx.commit()
                .await
                .context("failed to commit admin snapshot")?;
            Ok(verdi)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

async fn command_i_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    command_id: Uuid,
) -> Result<Option<AdminCommand>> {
    let Some(rad) = sqlx::query(
        r#"SELECT command_id, correlation_id, command_type, mottatt_at,
                  dispatchet_at, dekomponert_at
           FROM command
           WHERE command_id = $1"#,
    )
    .bind(command_id)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to load admin command")?
    else {
        return Ok(None);
    };

    Ok(Some(AdminCommand {
        command_id: rad.try_get("command_id")?,
        correlation_id: rad.try_get("correlation_id")?,
        command_type: rad.try_get("command_type")?,
        mottatt_at: rad.try_get("mottatt_at")?,
        dispatchet_at: rad.try_get("dispatchet_at")?,
        dekomponert_at: rad.try_get("dekomponert_at")?,
        operasjoner: hent_operasjonsdetaljer(tx, command_id).await?,
    }))
}

async fn hent_operasjonsdetaljer(
    tx: &mut Transaction<'_, Postgres>,
    command_id: Uuid,
) -> Result<Vec<AdminOperasjonDetaljer>> {
    let rader = sqlx::query(
        r#"SELECT o.operasjon_id, o.operasjonstype, o.entitet_id,
                  e.entitet_type::text AS entitet_type, e.client_reference, e.arkiv_id,
                  o.sak_id, o.status::text AS status, o.attempt_no, o.neste_forsok_at,
                  o.blokkert_av, o.siste_detalj, o.sendt_at, o.ferdig_at, o.varslet_at,
                  o.created_at, o.updated_at
           FROM operasjon o
           JOIN entitet e ON e.skuffen_id = o.entitet_id
           WHERE o.command_id = $1
           ORDER BY o.created_at, o.operasjon_id"#,
    )
    .bind(command_id)
    .fetch_all(&mut **tx)
    .await
    .context("failed to load admin operasjoner")?;

    rader
        .into_iter()
        .map(|rad| {
            Ok(AdminOperasjonDetaljer {
                operasjon_id: OperasjonId(rad.try_get("operasjon_id")?),
                operasjonstype: rad.try_get("operasjonstype")?,
                entitet: AdminOperasjonEntitet {
                    skuffen_id: entitet_id(
                        rad.try_get::<String, _>("entitet_type")?.as_str(),
                        rad.try_get("entitet_id")?,
                    )?,
                    client_reference: rad.try_get("client_reference")?,
                    arkiv_id: rad.try_get("arkiv_id")?,
                },
                sak_id: SkuffenSakId(rad.try_get("sak_id")?),
                status: rad.try_get("status")?,
                attempt_no: rad.try_get("attempt_no")?,
                neste_forsok_at: rad.try_get("neste_forsok_at")?,
                blokkert_av: rad.try_get("blokkert_av")?,
                siste_detalj: rad.try_get("siste_detalj")?,
                sendt_at: rad.try_get("sendt_at")?,
                ferdig_at: rad.try_get("ferdig_at")?,
                varslet_at: rad.try_get("varslet_at")?,
                created_at: rad.try_get("created_at")?,
                updated_at: rad.try_get("updated_at")?,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Sak
// ---------------------------------------------------------------------------

/// Fem navngitte SELECTs når `sak_tilstand` finnes, tre ved identity-only.
/// Antallet er uavhengig av hvor mange journalposter og dokumenter saken har.
async fn sak_i_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    key: AdminSakNokkel,
) -> Result<Option<AdminSak>> {
    let Some(identitet) = hent_sak_identitet(tx, key).await? else {
        return Ok(None);
    };
    let sak_id: Uuid = identitet.skuffen_id.as_uuid();

    let fakta = match hent_sak_fakta(tx, sak_id).await? {
        Some(mut fakta) => {
            let mut journalposter = hent_journalposter(tx, sak_id).await?;
            let mut dokumenter = hent_dokumenter_for_sak(tx, sak_id).await?;
            for journalpost in &mut journalposter {
                journalpost.dokumenter = dokumenter
                    .remove(&journalpost.identitet.skuffen_id.as_uuid())
                    .unwrap_or_default();
            }
            fakta.journalposter = journalposter;
            Some(fakta)
        }
        None => None,
    };

    Ok(Some(AdminSak {
        identitet,
        fakta,
        operasjoner: hent_operasjonssammendrag(tx, sak_id).await?,
    }))
}

async fn hent_sak_identitet(
    tx: &mut Transaction<'_, Postgres>,
    key: AdminSakNokkel,
) -> Result<Option<AdminEntitetIdentitet>> {
    const PER_SKUFFEN_ID: &str = r#"SELECT skuffen_id, entitet_type::text AS entitet_type,
                                           client_reference, arkiv_id, created_at, updated_at
                                    FROM entitet
                                    WHERE skuffen_id = $1 AND entitet_type = 'sak'"#;
    const PER_CLIENT_REFERENCE: &str = r#"SELECT skuffen_id, entitet_type::text AS entitet_type,
                                                 client_reference, arkiv_id, created_at, updated_at
                                          FROM entitet
                                          WHERE client_reference = $1 AND entitet_type = 'sak'"#;
    const PER_ARKIV_ID: &str = r#"SELECT skuffen_id, entitet_type::text AS entitet_type,
                                         client_reference, arkiv_id, created_at, updated_at
                                  FROM entitet
                                  WHERE arkiv_id = $1 AND entitet_type = 'sak'"#;

    let query = match key {
        AdminSakNokkel::SkuffenId(sak_id) => sqlx::query(PER_SKUFFEN_ID).bind(Uuid::from(sak_id)),
        AdminSakNokkel::ClientReference(client_reference) => {
            sqlx::query(PER_CLIENT_REFERENCE).bind(client_reference)
        }
        AdminSakNokkel::ArkivId(arkiv_id) => sqlx::query(PER_ARKIV_ID).bind(arkiv_id),
    };

    let rad = query
        .fetch_optional(&mut **tx)
        .await
        .context("failed to resolve admin sak identity")?;

    rad.map(|rad| entitet_identitet(&rad)).transpose()
}

async fn hent_sak_fakta(
    tx: &mut Transaction<'_, Postgres>,
    sak_id: Uuid,
) -> Result<Option<AdminSakFakta>> {
    let Some(rad) = sqlx::query(
        r#"SELECT tilstand, sakstittel, arkivdel, ordningsverdi,
                  saksbehandler_id AS opprettelse_saksbehandler_id,
                  saksbehandler_enhet AS opprettelse_saksbehandler_enhet,
                  tilgangskode, tilgangshjemmel,
                  oensket_saksansvarlig_id, oensket_saksansvarlig_enhet,
                  naavaerende_saksansvarlig_id, naavaerende_saksansvarlig_enhet,
                  opprettet_av_command_id, created_at, updated_at
           FROM sak_tilstand
           WHERE sak_id = $1"#,
    )
    .bind(sak_id)
    .fetch_optional(&mut **tx)
    .await
    .context("failed to load admin sak facts")?
    else {
        return Ok(None);
    };

    Ok(Some(AdminSakFakta {
        tilstand: rad.try_get("tilstand")?,
        sakstittel: rad.try_get("sakstittel")?,
        arkivdel: rad.try_get("arkivdel")?,
        ordningsverdi: rad.try_get("ordningsverdi")?,
        opprettelse_saksbehandler_id: rad.try_get("opprettelse_saksbehandler_id")?,
        opprettelse_saksbehandler_enhet: rad.try_get("opprettelse_saksbehandler_enhet")?,
        tilgangskode: rad.try_get("tilgangskode")?,
        tilgangshjemmel: rad.try_get("tilgangshjemmel")?,
        oensket_saksansvarlig_id: rad.try_get("oensket_saksansvarlig_id")?,
        oensket_saksansvarlig_enhet: rad.try_get("oensket_saksansvarlig_enhet")?,
        naavaerende_saksansvarlig_id: rad.try_get("naavaerende_saksansvarlig_id")?,
        naavaerende_saksansvarlig_enhet: rad.try_get("naavaerende_saksansvarlig_enhet")?,
        opprettet_av_command_id: rad.try_get("opprettet_av_command_id")?,
        created_at: rad.try_get("created_at")?,
        updated_at: rad.try_get("updated_at")?,
        journalposter: Vec::new(),
    }))
}

async fn hent_journalposter(
    tx: &mut Transaction<'_, Postgres>,
    sak_id: Uuid,
) -> Result<Vec<AdminJournalpost>> {
    let rader = sqlx::query(
        r#"SELECT j.journalpost_id, j.sak_id, j.tilstand, j.journalposttype, j.med_utsending,
                  j.tittel, j.dokument_dato, j.saksbehandler_id, j.saksbehandler_enhet,
                  j.tilgangskode, j.tilgangshjemmel, j.korrespondanseparter, j.kildesystem,
                  j.opprettet_av_command_id, j.created_at, j.updated_at,
                  e.skuffen_id, e.entitet_type::text AS entitet_type, e.client_reference,
                  e.arkiv_id, e.created_at AS entitet_created_at,
                  e.updated_at AS entitet_updated_at
           FROM journalpost_tilstand j
           JOIN entitet e ON e.skuffen_id = j.journalpost_id
           WHERE j.sak_id = $1
           ORDER BY j.created_at, j.journalpost_id"#,
    )
    .bind(sak_id)
    .fetch_all(&mut **tx)
    .await
    .context("failed to load admin journalposter")?;

    rader
        .into_iter()
        .map(|rad| {
            Ok(AdminJournalpost {
                identitet: entitet_identitet_med_prefiks(&rad)?,
                sak_id: SkuffenSakId(rad.try_get("sak_id")?),
                tilstand: rad.try_get("tilstand")?,
                journalposttype: rad.try_get("journalposttype")?,
                med_utsending: rad.try_get("med_utsending")?,
                tittel: rad.try_get("tittel")?,
                dokument_dato: rad.try_get("dokument_dato")?,
                saksbehandler_id: rad.try_get("saksbehandler_id")?,
                saksbehandler_enhet: rad.try_get("saksbehandler_enhet")?,
                tilgangskode: rad.try_get("tilgangskode")?,
                tilgangshjemmel: rad.try_get("tilgangshjemmel")?,
                korrespondanseparter: korrespondanseparter(rad.try_get("korrespondanseparter")?)?,
                kildesystem: rad.try_get("kildesystem")?,
                opprettet_av_command_id: rad.try_get("opprettet_av_command_id")?,
                created_at: rad.try_get("created_at")?,
                updated_at: rad.try_get("updated_at")?,
                dokumenter: Vec::new(),
            })
        })
        .collect()
}

/// Alle dokumenter for saken i ett statement. Grupperingen bevarer den
/// sorterte rekkefølgen; `HashMap`-iterasjon bestemmer ikke wire-order.
async fn hent_dokumenter_for_sak(
    tx: &mut Transaction<'_, Postgres>,
    sak_id: Uuid,
) -> Result<HashMap<Uuid, Vec<AdminDokument>>> {
    let rader = sqlx::query(
        r#"SELECT d.dokument_id, d.journalpost_id, d.tilstand, d.rekkefolge, d.er_hoveddokument,
                  d.tittel, d.filtype, d.dokument_referanse, d.mal_referanse, d.felter,
                  d.rendered_dokument_referanse, d.opprettet_av_command_id,
                  d.created_at, d.updated_at,
                  e.skuffen_id, e.entitet_type::text AS entitet_type, e.client_reference,
                  e.arkiv_id, e.created_at AS entitet_created_at,
                  e.updated_at AS entitet_updated_at
           FROM dokument_tilstand d
           JOIN journalpost_tilstand j ON j.journalpost_id = d.journalpost_id
           JOIN entitet e ON e.skuffen_id = d.dokument_id
           WHERE j.sak_id = $1
           ORDER BY d.journalpost_id, d.rekkefolge, d.dokument_id"#,
    )
    .bind(sak_id)
    .fetch_all(&mut **tx)
    .await
    .context("failed to load admin dokumenter")?;

    let mut gruppert: HashMap<Uuid, Vec<AdminDokument>> = HashMap::new();
    for rad in rader {
        let journalpost_id: Uuid = rad.try_get("journalpost_id")?;
        gruppert
            .entry(journalpost_id)
            .or_default()
            .push(AdminDokument {
                identitet: entitet_identitet_med_prefiks(&rad)?,
                journalpost_id,
                tilstand: rad.try_get("tilstand")?,
                rekkefolge: rad.try_get("rekkefolge")?,
                er_hoveddokument: rad.try_get("er_hoveddokument")?,
                tittel: rad.try_get("tittel")?,
                filtype: rad.try_get("filtype")?,
                dokument_referanse: rad.try_get("dokument_referanse")?,
                mal_referanse: rad.try_get("mal_referanse")?,
                felter: felter(rad.try_get("felter")?)?,
                rendered_dokument_referanse: rad.try_get("rendered_dokument_referanse")?,
                opprettet_av_command_id: rad.try_get("opprettet_av_command_id")?,
                created_at: rad.try_get("created_at")?,
                updated_at: rad.try_get("updated_at")?,
            });
    }
    Ok(gruppert)
}

async fn hent_operasjonssammendrag(
    tx: &mut Transaction<'_, Postgres>,
    sak_id: Uuid,
) -> Result<Vec<AdminOperasjonSammendrag>> {
    let rader = sqlx::query(
        r#"SELECT o.operasjon_id, o.command_id, o.operasjonstype, o.entitet_id,
                  e.entitet_type::text AS entitet_type, o.status::text AS status
           FROM operasjon o
           JOIN entitet e ON e.skuffen_id = o.entitet_id
           WHERE o.sak_id = $1
           ORDER BY o.created_at, o.operasjon_id"#,
    )
    .bind(sak_id)
    .fetch_all(&mut **tx)
    .await
    .context("failed to load admin operasjonssammendrag")?;

    rader
        .into_iter()
        .map(|rad| {
            Ok(AdminOperasjonSammendrag {
                operasjon_id: OperasjonId(rad.try_get("operasjon_id")?),
                command_id: rad.try_get("command_id")?,
                operasjonstype: rad.try_get("operasjonstype")?,
                entitet_id: entitet_id(
                    rad.try_get::<String, _>("entitet_type")?.as_str(),
                    rad.try_get("entitet_id")?,
                )?,
                status: rad.try_get("status")?,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Radoversettelse
// ---------------------------------------------------------------------------

fn entitet_id(entitet_type: &str, skuffen_id: Uuid) -> Result<EntitetId> {
    Ok(match entitet_type {
        "sak" => EntitetId::Sak(SkuffenSakId(skuffen_id)),
        "journalpost" => EntitetId::Journalpost(SkuffenJournalpostId(skuffen_id)),
        "dokument" => EntitetId::Dokument(SkuffenDokumentId(skuffen_id)),
        other => return Err(anyhow!("ukjent entitet_type: {other}")),
    })
}

fn entitet_identitet(rad: &sqlx::postgres::PgRow) -> Result<AdminEntitetIdentitet> {
    Ok(AdminEntitetIdentitet {
        skuffen_id: entitet_id(
            rad.try_get::<String, _>("entitet_type")?.as_str(),
            rad.try_get("skuffen_id")?,
        )?,
        client_reference: rad.try_get("client_reference")?,
        arkiv_id: rad.try_get("arkiv_id")?,
        created_at: rad.try_get("created_at")?,
        updated_at: rad.try_get("updated_at")?,
    })
}

/// Entitet-tidsstemplene er aliaset fordi state-tabellen har samme kolonnenavn.
fn entitet_identitet_med_prefiks(rad: &sqlx::postgres::PgRow) -> Result<AdminEntitetIdentitet> {
    Ok(AdminEntitetIdentitet {
        skuffen_id: entitet_id(
            rad.try_get::<String, _>("entitet_type")?.as_str(),
            rad.try_get("skuffen_id")?,
        )?,
        client_reference: rad.try_get("client_reference")?,
        arkiv_id: rad.try_get("arkiv_id")?,
        created_at: rad.try_get("entitet_created_at")?,
        updated_at: rad.try_get("entitet_updated_at")?,
    })
}

#[derive(Deserialize)]
struct KorrespondansepartJson {
    rolle: String,
    navn: String,
    #[serde(default)]
    parttype: Option<String>,
    #[serde(default)]
    id_type: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    adresse: Option<String>,
    #[serde(default)]
    postnummer: Option<String>,
    #[serde(default)]
    poststed: Option<String>,
}

/// `NULL` og `[]` bevares separat. Et lagret element som ikke har den avtalte
/// formen er en mappingfeil; admin read er ikke en rå JSONB-konsoll.
fn korrespondanseparter(
    verdi: Option<serde_json::Value>,
) -> Result<Option<Vec<AdminKorrespondansepart>>> {
    let Some(verdi) = verdi else {
        return Ok(None);
    };
    let liste: Vec<KorrespondansepartJson> =
        serde_json::from_value(verdi).context("korrespondanseparter har ukjent form")?;

    Ok(Some(
        liste
            .into_iter()
            .map(|part| AdminKorrespondansepart {
                rolle: part.rolle,
                navn: part.navn,
                parttype: part.parttype,
                id_type: part.id_type,
                id: part.id,
                adresse: part.adresse,
                postnummer: part.postnummer,
                poststed: part.poststed,
            })
            .collect(),
    ))
}

/// `NULL` og `[]` bevares separat. En lagret ikke-string token er en
/// mappingfeil, etter samme avgrensning som korrespondanse-JSON.
fn felter(verdi: Option<serde_json::Value>) -> Result<Option<Vec<String>>> {
    let Some(verdi) = verdi else {
        return Ok(None);
    };
    let tokens: Vec<String> =
        serde_json::from_value(verdi).context("felter er ikke en liste av tokens")?;
    Ok(Some(tokens))
}
