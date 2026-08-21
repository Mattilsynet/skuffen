use anyhow::{Context, Result, anyhow};
use application::command::materialisering::{
    DokumentAttributter, Dokumentkilde, JournalpostAttributter, Korrespondanseparter,
    SakAttributter, Tilgang,
};
use application::command::model::{
    Arkivdel, Korrespondansepart, MottakerId, Parttype, Postadresse, Utsendingsmottaker,
};
use application::command::ports::fakta_port::FaktaRepository;
use async_trait::async_trait;
use domain::eksekvering::TemplateFelt;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, JournalpostType, SakMedBarn, SakTilstand, Saksansvarlig,
};
use lib_sql::database_config::DbPool;
use serde::Deserialize;
use uuid::Uuid;

pub struct PostgresFaktaRepository {
    pool: DbPool,
}

impl PostgresFaktaRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

// ---------------------------------------------------------------------------
// Kodeoversettelse
// ---------------------------------------------------------------------------

fn sak_tilstand(kode: &str) -> Result<SakTilstand> {
    Ok(match kode {
        "ikke_opprettet" => SakTilstand::IkkeOpprettet,
        "opprettet" => SakTilstand::Opprettet,
        "avsluttet" => SakTilstand::Avsluttet,
        other => return Err(anyhow!("ukjent sak_tilstand: {other}")),
    })
}

fn journalpost_tilstand(kode: &str) -> Result<JournalpostTilstand> {
    Ok(match kode {
        "ikke_opprettet" => JournalpostTilstand::IkkeOpprettet,
        "opprettet" => JournalpostTilstand::Opprettet,
        "klar_for_ekspedering" => JournalpostTilstand::KlarForEkspedering,
        "ekspedert" => JournalpostTilstand::Ekspedert,
        "journalfoert" => JournalpostTilstand::Journalfoert,
        "avskrevet" => JournalpostTilstand::Avskrevet,
        other => return Err(anyhow!("ukjent journalpost_tilstand: {other}")),
    })
}

fn dokument_tilstand(kode: &str) -> Result<DokumentTilstand> {
    Ok(match kode {
        "avventer_rendring" => DokumentTilstand::AvventerRendring,
        "klar" => DokumentTilstand::Klar,
        "ok" => DokumentTilstand::Ok,
        other => return Err(anyhow!("ukjent dokument_tilstand: {other}")),
    })
}

fn journalposttype(kode: &str) -> Result<JournalpostType> {
    Ok(match kode {
        "I" => JournalpostType::Inngaende,
        "U" => JournalpostType::Utgaaende,
        "X" => JournalpostType::InterntNotat,
        other => return Err(anyhow!("ukjent journalposttype: {other}")),
    })
}

fn arkivdel(kode: &str) -> Result<Arkivdel> {
    Ok(match kode {
        "tilsynsdivisjonene" => Arkivdel::Tilsynsdivisjonene,
        "hovedkontoret" => Arkivdel::Hovedkontoret,
        other => return Err(anyhow!("ukjent arkivdel: {other}")),
    })
}

fn template_felt(token: &str) -> Result<TemplateFelt> {
    match token {
        "saksnummer" => Ok(TemplateFelt::Saksnummer),
        other => Err(anyhow!("ukjent template-felt: {other}")),
    }
}

fn felter_fra_json(verdi: Option<serde_json::Value>) -> Result<Vec<TemplateFelt>> {
    let Some(verdi) = verdi else {
        return Ok(Vec::new());
    };
    let tokens: Vec<String> =
        serde_json::from_value(verdi).context("felter er ikke en liste av tokens")?;
    tokens.iter().map(|token| template_felt(token)).collect()
}

// ---------------------------------------------------------------------------
// Korrespondanseparter
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(tag = "rolle", rename_all = "snake_case")]
enum KorrespondansepartJson {
    Avsender(PartJson),
    Mottaker(PartJson),
    Utsendingsmottaker(UtsendingsmottakerJson),
}

#[derive(Deserialize)]
struct PartJson {
    navn: String,
    parttype: String,
}

#[derive(Deserialize)]
struct UtsendingsmottakerJson {
    navn: String,
    id_type: String,
    id: String,
    adresse: String,
    postnummer: String,
    poststed: String,
}

fn korrespondanseparter(verdi: Option<serde_json::Value>) -> Result<Korrespondanseparter> {
    let Some(verdi) = verdi else {
        return Ok(Korrespondanseparter::Ingen);
    };
    let liste: Vec<KorrespondansepartJson> =
        serde_json::from_value(verdi).context("korrespondanseparter har ukjent form")?;

    if liste.is_empty() {
        return Ok(Korrespondanseparter::Ingen);
    }

    // Formen er homogen per journalpost, satt ved dekomponering.
    if let Some(KorrespondansepartJson::Avsender(_)) = liste.first() {
        let KorrespondansepartJson::Avsender(part) = &liste[0] else {
            unreachable!()
        };
        return Ok(Korrespondanseparter::Avsender(part_fra_json(part)?));
    }

    if matches!(liste[0], KorrespondansepartJson::Utsendingsmottaker(_)) {
        let mottakere = liste
            .iter()
            .map(|part| match part {
                KorrespondansepartJson::Utsendingsmottaker(m) => utsendingsmottaker(m),
                _ => Err(anyhow!("blandede korrespondansepartformer")),
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(Korrespondanseparter::Utsendingsmottakere(mottakere));
    }

    let mottakere = liste
        .iter()
        .map(|part| match part {
            KorrespondansepartJson::Mottaker(p) => part_fra_json(p),
            _ => Err(anyhow!("blandede korrespondansepartformer")),
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Korrespondanseparter::Mottakere(mottakere))
}

fn part_fra_json(part: &PartJson) -> Result<Korrespondansepart> {
    Ok(Korrespondansepart {
        navn: part.navn.clone(),
        parttype: match part.parttype.as_str() {
            "person" => Parttype::Person,
            "virksomhet" => Parttype::Virksomhet,
            other => return Err(anyhow!("ukjent parttype: {other}")),
        },
    })
}

fn utsendingsmottaker(mottaker: &UtsendingsmottakerJson) -> Result<Utsendingsmottaker> {
    let id = match mottaker.id_type.as_str() {
        "fodselsnummer" => MottakerId::Person {
            fødselsnummer: domain::model::identifikator::Fødselsnummer::new(&mottaker.id)
                .map_err(|e| anyhow!("ugyldig lagret fødselsnummer: {e}"))?,
        },
        "organisasjonsnummer" => MottakerId::Virksomhet {
            organisasjonsnummer: domain::model::identifikator::Organisasjonsnummer::new(
                &mottaker.id,
            )
            .map_err(|e| anyhow!("ugyldig lagret organisasjonsnummer: {e}"))?,
        },
        other => return Err(anyhow!("ukjent id_type: {other}")),
    };

    Ok(Utsendingsmottaker {
        navn: mottaker.navn.clone(),
        id,
        adresse: Postadresse {
            adresse: mottaker.adresse.clone(),
            postnummer: domain::model::identifikator::Postnummer::new(&mottaker.postnummer)
                .map_err(|e| anyhow!("ugyldig lagret postnummer: {e}"))?,
            poststed: mottaker.poststed.clone(),
        },
    })
}

// ---------------------------------------------------------------------------
// Dokumentrader
// ---------------------------------------------------------------------------

type DokumentRad = (
    Uuid,
    String,
    i32,
    Option<String>,
    Option<String>,
    Option<Uuid>,
    Option<Uuid>,
    Option<serde_json::Value>,
    Option<Uuid>,
);

fn dokument_attributter(rad: &DokumentRad) -> Result<(SkuffenDokumentId, DokumentAttributter)> {
    let (
        dokument_id,
        _tilstand,
        rekkefolge,
        tittel,
        filtype,
        dokument_referanse,
        mal_referanse,
        felter,
        rendered,
    ) = rad;

    let kilde = match (dokument_referanse, mal_referanse) {
        (Some(referanse), None) => Dokumentkilde::Bytes {
            dokument_referanse: *referanse,
            filtype: filtype.clone().unwrap_or_default(),
        },
        (None, Some(mal)) => Dokumentkilde::HtmlTemplate {
            mal_referanse: *mal,
            felter: felter_fra_json(felter.clone())?,
            rendered_dokument_referanse: *rendered,
        },
        _ => return Err(anyhow!("dokument har ugyldig kildeform")),
    };

    Ok((
        SkuffenDokumentId::from(*dokument_id),
        DokumentAttributter {
            tittel: tittel.clone().unwrap_or_default(),
            rekkefolge: u16::try_from(*rekkefolge).context("rekkefolge utenfor rekkevidde")?,
            kilde,
        },
    ))
}

fn dokument_med_tilstand(rad: &DokumentRad) -> Result<DokumentMedTilstand> {
    let (dokument_id, attributter) = dokument_attributter(rad)?;
    let kilde = match attributter.kilde {
        Dokumentkilde::Bytes { .. } => DokumentKildeTilstand::Bytes,
        Dokumentkilde::HtmlTemplate {
            mal_referanse,
            felter,
            rendered_dokument_referanse,
        } => DokumentKildeTilstand::HtmlTemplate {
            mal_referanse,
            felter,
            rendered_dokument_referanse,
        },
    };

    Ok(DokumentMedTilstand {
        dokument_id,
        tilstand: dokument_tilstand(&rad.1)?,
        rekkefolge: attributter.rekkefolge,
        kilde,
    })
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

#[async_trait]
impl FaktaRepository for PostgresFaktaRepository {
    /// Arkiv-id-er joines fra `entitet`, som er eneste sted de bor
    /// (SKU-0016 R11).
    async fn hent_sak_med_barn(&self, sak_id: SkuffenSakId) -> Result<Option<SakMedBarn>> {
        let sak_uuid: Uuid = sak_id.into();

        let sak: Option<(
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            r#"SELECT s.tilstand, e.arkiv_id,
                      s.oensket_saksansvarlig_id, s.oensket_saksansvarlig_enhet,
                      s.naavaerende_saksansvarlig_id, s.naavaerende_saksansvarlig_enhet
               FROM sak_tilstand s
               JOIN entitet e ON e.skuffen_id = s.sak_id
               WHERE s.sak_id = $1"#,
        )
        .bind(sak_uuid)
        .fetch_optional(&self.pool)
        .await
        .context("failed to load sak facts")?;

        let Some((tilstand, arkiv_id, oensket_id, oensket_enhet, naa_id, naa_enhet)) = sak else {
            return Ok(None);
        };

        let journalposter: Vec<(Uuid, String, Option<String>, String, bool)> = sqlx::query_as(
            r#"SELECT j.journalpost_id, j.tilstand, e.arkiv_id, j.journalposttype, j.med_utsending
               FROM journalpost_tilstand j
               JOIN entitet e ON e.skuffen_id = j.journalpost_id
               WHERE j.sak_id = $1
               ORDER BY j.created_at"#,
        )
        .bind(sak_uuid)
        .fetch_all(&self.pool)
        .await
        .context("failed to load journalpost facts")?;

        let mut barn = Vec::with_capacity(journalposter.len());
        for (journalpost_id, jp_tilstand, jp_arkiv_id, jp_type, med_utsending) in journalposter {
            let rader: Vec<DokumentRad> = sqlx::query_as(
                r#"SELECT dokument_id, tilstand, rekkefolge, tittel, filtype,
                          dokument_referanse, mal_referanse, felter,
                          rendered_dokument_referanse
                   FROM dokument_tilstand
                   WHERE journalpost_id = $1
                   ORDER BY rekkefolge"#,
            )
            .bind(journalpost_id)
            .fetch_all(&self.pool)
            .await
            .context("failed to load dokument facts")?;

            barn.push(JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_id),
                tilstand: journalpost_tilstand(&jp_tilstand)?,
                arkiv_id: jp_arkiv_id,
                journalposttype: journalposttype(&jp_type)?,
                med_utsending,
                dokumenter: rader
                    .iter()
                    .map(dokument_med_tilstand)
                    .collect::<Result<Vec<_>>>()?,
            });
        }

        Ok(Some(SakMedBarn {
            sak_id,
            tilstand: sak_tilstand(&tilstand)?,
            arkiv_id,
            oensket_saksansvarlig: saksansvarlig(oensket_id, oensket_enhet),
            naavaerende_saksansvarlig: saksansvarlig(naa_id, naa_enhet),
            journalposter: barn,
        }))
    }

    async fn hent_sak_attributter(&self, sak_id: SkuffenSakId) -> Result<Option<SakAttributter>> {
        let rad: Option<(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            r#"SELECT sakstittel, arkivdel, ordningsverdi, saksbehandler_id,
                      saksbehandler_enhet, tilgangskode, tilgangshjemmel
               FROM sak_tilstand WHERE sak_id = $1"#,
        )
        .bind(Uuid::from(sak_id))
        .fetch_optional(&self.pool)
        .await
        .context("failed to load sak attributes")?;

        let Some((
            sakstittel,
            arkivdel_kode,
            ordningsverdi,
            saksbehandler_id,
            saksbehandler_enhet,
            tilgangskode,
            tilgangshjemmel,
        )) = rad
        else {
            return Ok(None);
        };

        // En sak vi ikke opprettet har ingen attributter.
        let (Some(sakstittel), Some(arkivdel_kode), Some(ordningsverdi)) =
            (sakstittel, arkivdel_kode, ordningsverdi)
        else {
            return Ok(None);
        };

        Ok(Some(SakAttributter {
            sakstittel,
            arkivdel: arkivdel(&arkivdel_kode)?,
            ordningsverdi,
            saksbehandler_id: saksbehandler_id.unwrap_or_default(),
            saksbehandler_enhet: saksbehandler_enhet.unwrap_or_default(),
            tilgang: Tilgang {
                tilgangskode,
                tilgangshjemmel,
            },
        }))
    }

    async fn hent_journalpost_attributter(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostAttributter>> {
        let rad: Option<(
            String,
            String,
            bool,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<serde_json::Value>,
            Option<String>,
            Option<Uuid>,
        )> = sqlx::query_as(
            r#"SELECT j.journalposttype, j.tittel, j.med_utsending, j.dokument_dato,
                      j.saksbehandler_id, j.saksbehandler_enhet, j.tilgangskode, j.tilgangshjemmel,
                      j.korrespondanseparter, j.kildesystem, e.client_reference
               FROM journalpost_tilstand j
               JOIN entitet e ON e.skuffen_id = j.journalpost_id
               WHERE j.journalpost_id = $1"#,
        )
        .bind(Uuid::from(journalpost_id))
        .fetch_optional(&self.pool)
        .await
        .context("failed to load journalpost attributes")?;

        let Some((
            jp_type,
            tittel,
            med_utsending,
            dokument_dato,
            saksbehandler_id,
            saksbehandler_enhet,
            tilgangskode,
            tilgangshjemmel,
            parter,
            kildesystem,
            client_reference,
        )) = rad
        else {
            return Ok(None);
        };

        Ok(Some(JournalpostAttributter {
            client_reference: client_reference
                .ok_or_else(|| anyhow!("journalpost mangler client_reference"))?,
            tittel,
            dokument_dato: dokument_dato.unwrap_or_default(),
            journalposttype: journalposttype(&jp_type)?,
            med_utsending,
            saksbehandler_id: saksbehandler_id.unwrap_or_default(),
            saksbehandler_enhet: saksbehandler_enhet.unwrap_or_default(),
            tilgang: Tilgang {
                tilgangskode,
                tilgangshjemmel,
            },
            korrespondanseparter: korrespondanseparter(parter)?,
            kildesystem,
        }))
    }

    async fn hent_dokument_attributter(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentAttributter>> {
        let rad: Option<DokumentRad> = sqlx::query_as(
            r#"SELECT dokument_id, tilstand, rekkefolge, tittel, filtype,
                      dokument_referanse, mal_referanse, felter,
                      rendered_dokument_referanse
               FROM dokument_tilstand
               WHERE dokument_id = $1"#,
        )
        .bind(Uuid::from(dokument_id))
        .fetch_optional(&self.pool)
        .await
        .context("failed to load dokument attributes")?;

        rad.map(|rad| dokument_attributter(&rad).map(|(_, attributter)| attributter))
            .transpose()
    }

    async fn hent_dokumenter_for_journalpost(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Vec<(SkuffenDokumentId, DokumentAttributter)>> {
        let rader: Vec<DokumentRad> = sqlx::query_as(
            r#"SELECT dokument_id, tilstand, rekkefolge, tittel, filtype,
                      dokument_referanse, mal_referanse, felter,
                      rendered_dokument_referanse
               FROM dokument_tilstand
               WHERE journalpost_id = $1
               ORDER BY rekkefolge"#,
        )
        .bind(Uuid::from(journalpost_id))
        .fetch_all(&self.pool)
        .await
        .context("failed to load dokumenter for journalpost")?;

        rader.iter().map(dokument_attributter).collect()
    }
}

fn saksansvarlig(id: Option<String>, enhet: Option<String>) -> Option<Saksansvarlig> {
    match (id, enhet) {
        (Some(saksbehandler_id), Some(enhet)) => Some(Saksansvarlig {
            saksbehandler_id,
            enhet,
        }),
        _ => None,
    }
}
