use anyhow::{Context, anyhow};
use application::command::ports::entity_tilstand_port::EntityTilstandRepository;
use async_trait::async_trait;
use domain::eksekvering::html_template::TemplateFelt;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, SakMedBarn, SakTilstand, Saksansvarlig,
};
use sqlx::Row;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use tracing::info;
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
        DokumentTilstand::AvventerRendring => "avventer_rendring",
        DokumentTilstand::Ok => "ok",
        DokumentTilstand::FeiletPermanent => "feilet_permanent",
    }
}

fn dokument_tilstand_from_db(s: &str) -> Result<DokumentTilstand, anyhow::Error> {
    match s {
        "ikke_realisert" => Ok(DokumentTilstand::IkkeRealisert),
        "avventer_rendring" => Ok(DokumentTilstand::AvventerRendring),
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

fn template_felter_to_db(felter: &[TemplateFelt]) -> Vec<&'static str> {
    felter.iter().map(|felt| felt.as_token()).collect()
}

fn template_felter_from_db(value: serde_json::Value) -> Result<Vec<TemplateFelt>, anyhow::Error> {
    let felter: Vec<String> = serde_json::from_value(value).context("deserialize felter")?;
    felter
        .into_iter()
        .map(|felt| match felt.as_str() {
            "Saksnummer" | "saksnummer" => Ok(TemplateFelt::Saksnummer),
            other => Err(anyhow!("Ukjent template_felt: {other}")),
        })
        .collect()
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
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO sak_tilstand (sak_id, tilstand, opprettet_av_command_id)
            VALUES ($1, 'ikke_realisert', $2)
            ON CONFLICT (sak_id) DO NOTHING
            "#,
        )
        .bind(Uuid::from(sak_id))
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
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE sak_tilstand
            SET tilstand = $2, sikri_id = $3, saksnummer = $4
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(sak_tilstand_to_db(tilstand))
        .bind(sikri_id)
        .bind(saksnummer)
        .execute(&self.pool)
        .await
        .context("oppdater_sak_tilstand")?;

        Ok(())
    }

    async fn ensure_sak_tilstand_for_arkiv_id(
        &self,
        sak_id: SkuffenSakId,
        saksnummer: &str,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        // command_id er lokal persistence-/audit-proveniens for første materialisering.
        sqlx::query(
            r#"
            INSERT INTO sak_tilstand (sak_id, tilstand, saksnummer, opprettet_av_command_id)
            VALUES ($1, 'opprettet', $2, $3)
            ON CONFLICT (sak_id) DO NOTHING
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(saksnummer)
        .bind(command_id)
        .execute(&self.pool)
        .await
        .context("ensure_sak_tilstand_for_arkiv_id")?;

        Ok(())
    }

    async fn oppdater_oensket_saksansvarlig(
        &self,
        sak_id: SkuffenSakId,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE sak_tilstand
            SET oensket_saksansvarlig_id = $2, oensket_saksansvarlig_enhet = $3
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(saksbehandler_id)
        .bind(saksbehandler_enhet)
        .execute(&self.pool)
        .await
        .context("oppdater_oensket_saksansvarlig")?;

        Ok(())
    }

    async fn oppdater_naavaerende_saksansvarlig(
        &self,
        sak_id: SkuffenSakId,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE sak_tilstand
            SET naavaerende_saksansvarlig_id = $2, naavaerende_saksansvarlig_enhet = $3
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
        .bind(saksbehandler_id)
        .bind(saksbehandler_enhet)
        .execute(&self.pool)
        .await
        .context("oppdater_naavaerende_saksansvarlig")?;

        Ok(())
    }

    async fn opprett_journalpost_tilstand(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        journalposttype: JournalpostType,
        med_utsending: bool,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO journalpost_tilstand
                (journalpost_id, sak_id, tilstand, journalposttype, med_utsending, opprettet_av_command_id)
            VALUES ($1, $2, 'ikke_realisert', $3, $4, $5)
            "#,
        )
        .bind(Uuid::from(journalpost_id))
        .bind(Uuid::from(sak_id))
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
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE journalpost_tilstand
            SET tilstand = $2, sikri_id = $3, journalpostnummer = $4
            WHERE journalpost_id = $1
            "#,
        )
        .bind(Uuid::from(journalpost_id))
        .bind(journalpost_tilstand_to_db(tilstand))
        .bind(sikri_id)
        .bind(journalpostnummer)
        .execute(&self.pool)
        .await
        .context("oppdater_journalpost_tilstand")?;

        Ok(())
    }

    async fn hent_sak_id_fra_journalpost_id(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        let row = sqlx::query(
            r#"
            SELECT sak_id
            FROM journalpost_tilstand
            WHERE journalpost_id = $1
            "#,
        )
        .bind(Uuid::from(journalpost_id))
        .fetch_optional(&self.pool)
        .await
        .context("hent_sak_id_fra_journalpost_id")?;

        Ok(row.map(|row| SkuffenSakId::from(row.get::<Uuid, _>("sak_id"))))
    }

    async fn opprett_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        tilstand: DokumentTilstand,
        mal_referanse: Option<Uuid>,
        felter: Vec<TemplateFelt>,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        let felter_json = if mal_referanse.is_some() {
            Some(serde_json::to_value(template_felter_to_db(&felter)).context("serialize felter")?)
        } else {
            None
        };
        sqlx::query(
            r#"
            INSERT INTO dokument_tilstand
                (dokument_id, journalpost_id, tilstand, mal_referanse, felter, opprettet_av_command_id)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .bind(Uuid::from(journalpost_id))
        .bind(dokument_tilstand_to_db(tilstand))
        .bind(mal_referanse)
        .bind(felter_json)
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
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE dokument_tilstand
            SET tilstand = $2
            WHERE dokument_id = $1
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .bind(dokument_tilstand_to_db(tilstand))
        .execute(&self.pool)
        .await
        .context("oppdater_dokument_tilstand")?;

        Ok(())
    }

    async fn hent_journalpost_id_fra_dokument_id(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        let row = sqlx::query(
            r#"
            SELECT journalpost_id
            FROM dokument_tilstand
            WHERE dokument_id = $1
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .fetch_optional(&self.pool)
        .await
        .context("hent_journalpost_id_fra_dokument_id")?;

        Ok(row.map(|row| SkuffenJournalpostId::from(row.get::<Uuid, _>("journalpost_id"))))
    }

    async fn oppdater_rendered_dokument_referanse(
        &self,
        dokument_id: SkuffenDokumentId,
        rendered_dokument_referanse: Uuid,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE dokument_tilstand
            SET rendered_dokument_referanse = $2
            WHERE dokument_id = $1
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .bind(rendered_dokument_referanse)
        .execute(&self.pool)
        .await
        .context("oppdater_rendered_dokument_referanse")?;

        Ok(())
    }

    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error> {
        let rows = sqlx::query(
            r#"
            SELECT
                s.sak_id, s.tilstand as sak_tilstand,
                s.sikri_id as sak_sikri_id, s.saksnummer,
                s.oensket_saksansvarlig_id, s.oensket_saksansvarlig_enhet,
                s.naavaerende_saksansvarlig_id, s.naavaerende_saksansvarlig_enhet,
                j.journalpost_id, j.tilstand as jp_tilstand,
                j.sikri_id as jp_sikri_id, j.journalpostnummer, j.journalposttype, j.med_utsending,
                d.dokument_id, d.tilstand as dok_tilstand, d.mal_referanse,
                d.felter, d.rendered_dokument_referanse
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
        let sak_sikri_id: Option<i64> = first.get("sak_sikri_id");
        let saksnummer: Option<String> = first.get("saksnummer");

        let oensket_saksansvarlig_id: Option<String> = first.get("oensket_saksansvarlig_id");
        let oensket_saksansvarlig_enhet: Option<String> = first.get("oensket_saksansvarlig_enhet");
        let naavaerende_saksansvarlig_id: Option<String> =
            first.get("naavaerende_saksansvarlig_id");
        let naavaerende_saksansvarlig_enhet: Option<String> =
            first.get("naavaerende_saksansvarlig_enhet");

        let oensket_saksansvarlig = oensket_saksansvarlig_id
            .zip(oensket_saksansvarlig_enhet)
            .map(|(id, enhet)| Saksansvarlig {
                saksbehandler_id: id,
                enhet,
            });
        let naavaerende_saksansvarlig = naavaerende_saksansvarlig_id
            .zip(naavaerende_saksansvarlig_enhet)
            .map(|(id, enhet)| Saksansvarlig {
                saksbehandler_id: id,
                enhet,
            });

        let mut journalposter: HashMap<Uuid, JournalpostMedDokumenter> = HashMap::new();

        for row in &rows {
            let jp_id: Option<Uuid> = row.get("journalpost_id");
            let Some(jp_id) = jp_id else {
                continue;
            };

            let jp = journalposter.entry(jp_id).or_insert_with(|| {
                // These unwraps are safe: if jp_id is Some, the join matched
                let jp_tilstand_str: &str = row.get("jp_tilstand");
                let jp_type_str: &str = row.get("journalposttype");

                JournalpostMedDokumenter {
                    journalpost_id: SkuffenJournalpostId::from(jp_id),
                    tilstand: journalpost_tilstand_from_db(jp_tilstand_str)
                        .expect("ugyldig jp_tilstand i db"),
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
                    let mal_referanse: Option<Uuid> = row.get("mal_referanse");
                    let felter_json: Option<serde_json::Value> = row.get("felter");
                    let rendered_dokument_referanse: Option<Uuid> =
                        row.get("rendered_dokument_referanse");
                    let felter = match felter_json {
                        Some(value) => template_felter_from_db(value).unwrap_or_default(),
                        None => Vec::new(),
                    };
                    let kilde = if let Some(mal_referanse) = mal_referanse {
                        DokumentKildeTilstand::HtmlTemplate {
                            mal_referanse,
                            felter,
                            rendered_dokument_referanse,
                        }
                    } else {
                        DokumentKildeTilstand::Bytes
                    };
                    jp.dokumenter.push(DokumentMedTilstand {
                        dokument_id: SkuffenDokumentId::from(dok_id),
                        tilstand: dok_tilstand,
                        kilde,
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
            sikri_id: sak_sikri_id,
            saksnummer,
            oensket_saksansvarlig,
            naavaerende_saksansvarlig,
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

        info!(
            entity_type = %entity_type,
            entity_id = %entity_id,
            command_id = %command_id,
            from_status = %fra_tilstand,
            to_status = %til_tilstand,
            operation = %operasjon,
            has_detail = feil_detalj.is_some(),
            "entity_state_transition"
        );

        Ok(())
    }
}
