use anyhow::{Context, anyhow};
use application::command::ports::entity_tilstand_port::EntityTilstandRepository;
use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter, JournalpostTilstand,
    SakMedBarn, SakTilstand,
};
use sqlx::Row;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// DB ↔ domain enum conversions (infra-local, not on domain types)
// ---------------------------------------------------------------------------

fn sak_tilstand_to_db(t: SakTilstand) -> &'static str {
    match t {
        SakTilstand::IkkeRealisert => "ikke_realisert",
        SakTilstand::Opprettet => "opprettet",
        SakTilstand::Avsluttet => "avsluttet",
        SakTilstand::FeiletPermanent => "feilet_permanent",
    }
}

fn sak_tilstand_from_db(s: &str) -> Result<SakTilstand, anyhow::Error> {
    match s {
        "ikke_realisert" => Ok(SakTilstand::IkkeRealisert),
        "opprettet" => Ok(SakTilstand::Opprettet),
        "avsluttet" => Ok(SakTilstand::Avsluttet),
        "feilet_permanent" => Ok(SakTilstand::FeiletPermanent),
        other => Err(anyhow!("Ukjent sak_tilstand: {other}")),
    }
}

fn journalpost_tilstand_to_db(t: JournalpostTilstand) -> &'static str {
    match t {
        JournalpostTilstand::IkkeRealisert => "ikke_realisert",
        JournalpostTilstand::Opprettet => "opprettet",
        JournalpostTilstand::DokumenterUnderArbeid => "dokumenter_under_arbeid",
        JournalpostTilstand::KlarForJournalforing => "klar_for_journalforing",
        JournalpostTilstand::VenterPaaUtsending => "venter_paa_utsending",
        JournalpostTilstand::Journalfoert => "journalfoert",
        JournalpostTilstand::Avskrevet => "avskrevet",
        JournalpostTilstand::FeiletPermanent => "feilet_permanent",
    }
}

fn journalpost_tilstand_from_db(s: &str) -> Result<JournalpostTilstand, anyhow::Error> {
    match s {
        "ikke_realisert" => Ok(JournalpostTilstand::IkkeRealisert),
        "opprettet" => Ok(JournalpostTilstand::Opprettet),
        "dokumenter_under_arbeid" => Ok(JournalpostTilstand::DokumenterUnderArbeid),
        "klar_for_journalforing" => Ok(JournalpostTilstand::KlarForJournalforing),
        "venter_paa_utsending" => Ok(JournalpostTilstand::VenterPaaUtsending),
        "journalfoert" => Ok(JournalpostTilstand::Journalfoert),
        "avskrevet" => Ok(JournalpostTilstand::Avskrevet),
        "feilet_permanent" => Ok(JournalpostTilstand::FeiletPermanent),
        other => Err(anyhow!("Ukjent journalpost_tilstand: {other}")),
    }
}

fn dokument_tilstand_to_db(t: DokumentTilstand) -> &'static str {
    match t {
        DokumentTilstand::IkkeRealisert => "ikke_realisert",
        DokumentTilstand::Ok => "ok",
        DokumentTilstand::FeiletPermanent => "feilet_permanent",
    }
}

fn dokument_tilstand_from_db(s: &str) -> Result<DokumentTilstand, anyhow::Error> {
    match s {
        "ikke_realisert" => Ok(DokumentTilstand::IkkeRealisert),
        "ok" => Ok(DokumentTilstand::Ok),
        "feilet_permanent" => Ok(DokumentTilstand::FeiletPermanent),
        other => Err(anyhow!("Ukjent dokument_tilstand: {other}")),
    }
}

fn journalposttype_to_db(t: JournalpostType) -> &'static str {
    match t {
        JournalpostType::Inngaende => "I",
        JournalpostType::Utgaaende => "U",
        JournalpostType::InterntNotat => "X",
    }
}

fn journalposttype_from_db(s: &str) -> Result<JournalpostType, anyhow::Error> {
    match s {
        "I" => Ok(JournalpostType::Inngaende),
        "U" => Ok(JournalpostType::Utgaaende),
        "X" => Ok(JournalpostType::InterntNotat),
        other => Err(anyhow!("Ukjent journalposttype: {other}")),
    }
}

// ---------------------------------------------------------------------------
// PostgresEntityTilstandStore
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct PostgresEntityTilstandStore {
    pool: PgPool,
}

impl PostgresEntityTilstandStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EntityTilstandRepository for PostgresEntityTilstandStore {
    async fn opprett_sak_tilstand(
        &self,
        sak_id: SkuffenSakId,
        oensket_tilstand: SakTilstand,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO sak_tilstand (sak_id, tilstand, oensket_tilstand, opprettet_av_command_id)
            VALUES ($1, 'ikke_realisert', $2, $3)
            ON CONFLICT (sak_id) DO NOTHING
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(sak_tilstand_to_db(oensket_tilstand))
        .bind(command_id)
        .execute(&self.pool)
        .await
        .context("opprett_sak_tilstand")?;

        Ok(())
    }

    async fn oppdater_sak_tilstand(
        &self,
        sak_id: SkuffenSakId,
        tilstand: SakTilstand,
        sikri_id: Option<i64>,
        saksnummer: Option<&str>,
        feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE sak_tilstand
            SET tilstand = $2, sikri_id = $3, saksnummer = $4, feil_detalj = $5
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(sak_tilstand_to_db(tilstand))
        .bind(sikri_id)
        .bind(saksnummer)
        .bind(feil_detalj)
        .execute(&self.pool)
        .await
        .context("oppdater_sak_tilstand")?;

        Ok(())
    }

    async fn oppdater_sak_oensket_tilstand(
        &self,
        sak_id: SkuffenSakId,
        oensket_tilstand: SakTilstand,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE sak_tilstand
            SET oensket_tilstand = $2
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(sak_tilstand_to_db(oensket_tilstand))
        .execute(&self.pool)
        .await
        .context("oppdater_sak_oensket_tilstand")?;

        Ok(())
    }

    async fn opprett_journalpost_tilstand(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        journalposttype: JournalpostType,
        med_utsending: bool,
        oensket_tilstand: JournalpostTilstand,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO journalpost_tilstand
                (journalpost_id, sak_id, tilstand, oensket_tilstand, journalposttype, med_utsending, opprettet_av_command_id)
            VALUES ($1, $2, 'ikke_realisert', $3, $4, $5, $6)
            "#,
        )
        .bind(Uuid::from(journalpost_id))
        .bind(Uuid::from(sak_id))
        .bind(journalpost_tilstand_to_db(oensket_tilstand))
        .bind(journalposttype_to_db(journalposttype))
        .bind(med_utsending)
        .bind(command_id)
        .execute(&self.pool)
        .await
        .context("opprett_journalpost_tilstand")?;

        Ok(())
    }

    async fn oppdater_journalpost_tilstand(
        &self,
        journalpost_id: SkuffenJournalpostId,
        tilstand: JournalpostTilstand,
        sikri_id: Option<i64>,
        journalpostnummer: Option<i32>,
        feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE journalpost_tilstand
            SET tilstand = $2, sikri_id = $3, journalpostnummer = $4, feil_detalj = $5
            WHERE journalpost_id = $1
            "#,
        )
        .bind(Uuid::from(journalpost_id))
        .bind(journalpost_tilstand_to_db(tilstand))
        .bind(sikri_id)
        .bind(journalpostnummer)
        .bind(feil_detalj)
        .execute(&self.pool)
        .await
        .context("oppdater_journalpost_tilstand")?;

        Ok(())
    }

    async fn opprett_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO dokument_tilstand (dokument_id, journalpost_id, tilstand, oensket_tilstand, opprettet_av_command_id)
            VALUES ($1, $2, 'ikke_realisert', 'ok', $3)
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .bind(Uuid::from(journalpost_id))
        .bind(command_id)
        .execute(&self.pool)
        .await
        .context("opprett_dokument_tilstand")?;

        Ok(())
    }

    async fn oppdater_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        tilstand: DokumentTilstand,
        feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE dokument_tilstand
            SET tilstand = $2, feil_detalj = $3
            WHERE dokument_id = $1
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .bind(dokument_tilstand_to_db(tilstand))
        .bind(feil_detalj)
        .execute(&self.pool)
        .await
        .context("oppdater_dokument_tilstand")?;

        Ok(())
    }

    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error> {
        let rows = sqlx::query(
            r#"
            SELECT
                s.sak_id, s.tilstand as sak_tilstand, s.oensket_tilstand as sak_oensket_tilstand,
                s.sikri_id as sak_sikri_id, s.saksnummer,
                j.journalpost_id, j.tilstand as jp_tilstand, j.oensket_tilstand as jp_oensket_tilstand,
                j.sikri_id as jp_sikri_id, j.journalpostnummer, j.journalposttype, j.med_utsending,
                d.dokument_id, d.tilstand as dok_tilstand
            FROM sak_tilstand s
            LEFT JOIN journalpost_tilstand j ON j.sak_id = s.sak_id
            LEFT JOIN dokument_tilstand d ON d.journalpost_id = j.journalpost_id
            WHERE s.sak_id = $1
            ORDER BY j.journalpost_id, d.dokument_id
            "#,
        )
        .bind(Uuid::from(sak_id))
        .fetch_all(&self.pool)
        .await
        .context("hent_sak_med_barn")?;

        if rows.is_empty() {
            return Ok(None);
        }

        let first = &rows[0];
        let sak_tilstand = sak_tilstand_from_db(first.get::<&str, _>("sak_tilstand"))?;
        let sak_oensket = sak_tilstand_from_db(first.get::<&str, _>("sak_oensket_tilstand"))?;
        let sak_sikri_id: Option<i64> = first.get("sak_sikri_id");
        let saksnummer: Option<String> = first.get("saksnummer");

        let mut journalposter: HashMap<Uuid, JournalpostMedDokumenter> = HashMap::new();

        for row in &rows {
            let jp_id: Option<Uuid> = row.get("journalpost_id");
            let Some(jp_id) = jp_id else {
                continue;
            };

            let jp = journalposter.entry(jp_id).or_insert_with(|| {
                // These unwraps are safe: if jp_id is Some, the join matched
                let jp_tilstand_str: &str = row.get("jp_tilstand");
                let jp_oensket_str: &str = row.get("jp_oensket_tilstand");
                let jp_type_str: &str = row.get("journalposttype");

                JournalpostMedDokumenter {
                    journalpost_id: SkuffenJournalpostId::from(jp_id),
                    tilstand: journalpost_tilstand_from_db(jp_tilstand_str)
                        .expect("ugyldig jp_tilstand i db"),
                    oensket_tilstand: journalpost_tilstand_from_db(jp_oensket_str)
                        .expect("ugyldig jp_oensket_tilstand i db"),
                    sikri_id: row.get("jp_sikri_id"),
                    journalpostnummer: row.get("journalpostnummer"),
                    journalposttype: journalposttype_from_db(jp_type_str)
                        .expect("ugyldig journalposttype i db"),
                    med_utsending: row.get("med_utsending"),
                    dokumenter: Vec::new(),
                }
            });

            let dok_id: Option<Uuid> = row.get("dokument_id");
            if let Some(dok_id) = dok_id {
                let dok_tilstand_str: &str = row.get("dok_tilstand");
                let dok_tilstand =
                    dokument_tilstand_from_db(dok_tilstand_str).expect("ugyldig dok_tilstand i db");

                // Avoid duplicates from the flat join
                let already_added = jp.dokumenter.iter().any(|d| d.dokument_id.0 == dok_id);
                if !already_added {
                    jp.dokumenter.push(DokumentMedTilstand {
                        dokument_id: SkuffenDokumentId::from(dok_id),
                        tilstand: dok_tilstand,
                    });
                }
            }
        }

        let mut journalposter: Vec<JournalpostMedDokumenter> =
            journalposter.into_values().collect();
        journalposter.sort_by_key(|jp| jp.journalpost_id.0);

        Ok(Some(SakMedBarn {
            sak_id,
            tilstand: sak_tilstand,
            oensket_tilstand: sak_oensket,
            sikri_id: sak_sikri_id,
            saksnummer,
            journalposter,
        }))
    }

    async fn logg_overgang(
        &self,
        entity_type: &str,
        entity_id: Uuid,
        command_id: Uuid,
        fra_tilstand: &str,
        til_tilstand: &str,
        operasjon: &str,
        feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO tilstand_historikk (entity_type, entity_id, command_id, fra_tilstand, til_tilstand, operasjon, feil_detalj)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(entity_type)
        .bind(entity_id)
        .bind(command_id)
        .bind(fra_tilstand)
        .bind(til_tilstand)
        .bind(operasjon)
        .bind(feil_detalj)
        .execute(&self.pool)
        .await
        .context("logg_overgang")?;

        Ok(())
    }
}
