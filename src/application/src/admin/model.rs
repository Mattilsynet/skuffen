//! Interne admin-modeller.
//!
//! Modellen speiler projectionen slik databasen faktisk lagrer den. Lagrede
//! koder og fritekst beholdes som strings, slik at historisk eller
//! reparasjonstrengende state kan vises uten å bli revalidert av
//! command-side typer.

use chrono::{DateTime, Utc};
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::{EntitetId, OperasjonId};
use uuid::Uuid;

/// Lagrede operasjonsstatuser som `utled_utfall` folder over.
const STATUS_FEILET: &str = "feilet";
const STATUS_KREVER_AVKLARING: &str = "krever_avklaring";
const STATUS_OK: &str = "ok";

/// Snapshot-sammendrag utledet bare fra nåværende operasjonsrader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminCommandUtfall {
    Uavklart,
    KreverAvklaring,
    Fullfort,
    Feilet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminEntitetIdentitet {
    pub skuffen_id: EntitetId,
    pub client_reference: Option<Uuid>,
    pub arkiv_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Kompakt entitet-identitet på en operasjonsrad.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminOperasjonEntitet {
    pub skuffen_id: EntitetId,
    pub client_reference: Option<Uuid>,
    pub arkiv_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminOperasjonDetaljer {
    pub operasjon_id: OperasjonId,
    pub operasjonstype: String,
    pub entitet: AdminOperasjonEntitet,
    pub sak_id: SkuffenSakId,
    pub status: String,
    pub attempt_no: i32,
    pub neste_forsok_at: Option<DateTime<Utc>>,
    pub blokkert_av: Option<Uuid>,
    pub siste_detalj: Option<String>,
    pub sendt_at: Option<DateTime<Utc>>,
    pub ferdig_at: Option<DateTime<Utc>>,
    pub varslet_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminCommand {
    pub command_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub command_type: String,
    pub mottatt_at: DateTime<Utc>,
    pub dispatchet_at: Option<DateTime<Utc>>,
    pub dekomponert_at: Option<DateTime<Utc>>,
    pub operasjoner: Vec<AdminOperasjonDetaljer>,
}

impl AdminCommand {
    /// Folder nåværende operasjonsstatuser til ett sammendrag.
    ///
    /// Prioriteten er `feilet` > `krever_avklaring` > `fullfort` > `uavklart`.
    /// En tom operasjonsliste er alltid `uavklart`, aldri vacuous `fullfort`:
    /// valideringsavvisning persisteres ikke lokalt, så fravær av operasjoner
    /// beviser ingenting om hvorfor kommandoen ikke har kommet videre.
    pub fn utled_utfall(&self) -> AdminCommandUtfall {
        if self.operasjoner.is_empty() {
            return AdminCommandUtfall::Uavklart;
        }

        if self
            .operasjoner
            .iter()
            .any(|operasjon| operasjon.status == STATUS_FEILET)
        {
            return AdminCommandUtfall::Feilet;
        }

        if self
            .operasjoner
            .iter()
            .any(|operasjon| operasjon.status == STATUS_KREVER_AVKLARING)
        {
            return AdminCommandUtfall::KreverAvklaring;
        }

        if self
            .operasjoner
            .iter()
            .all(|operasjon| operasjon.status == STATUS_OK)
        {
            return AdminCommandUtfall::Fullfort;
        }

        AdminCommandUtfall::Uavklart
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminKorrespondansepart {
    pub rolle: String,
    pub navn: String,
    pub parttype: Option<String>,
    pub id_type: Option<String>,
    pub id: Option<String>,
    pub adresse: Option<String>,
    pub postnummer: Option<String>,
    pub poststed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminDokument {
    pub identitet: AdminEntitetIdentitet,
    pub journalpost_id: Uuid,
    pub tilstand: String,
    pub rekkefolge: i32,
    pub er_hoveddokument: bool,
    pub tittel: Option<String>,
    pub filtype: Option<String>,
    pub dokument_referanse: Option<Uuid>,
    pub mal_referanse: Option<Uuid>,
    /// `None` for SQL `NULL`, `Some(vec![])` for lagret tom liste.
    pub felter: Option<Vec<String>>,
    pub rendered_dokument_referanse: Option<Uuid>,
    pub opprettet_av_command_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminJournalpost {
    pub identitet: AdminEntitetIdentitet,
    pub sak_id: SkuffenSakId,
    pub tilstand: String,
    pub journalposttype: String,
    pub med_utsending: bool,
    pub tittel: Option<String>,
    pub dokument_dato: Option<String>,
    /// Journalpostens egen saksbehandler. Et annet begrep enn sakens
    /// opprettelses-saksbehandler og enn saksansvarlig.
    pub saksbehandler_id: Option<String>,
    pub saksbehandler_enhet: Option<String>,
    pub tilgangskode: Option<String>,
    pub tilgangshjemmel: Option<String>,
    /// `None` for SQL `NULL`, `Some(vec![])` for lagret tom liste.
    pub korrespondanseparter: Option<Vec<AdminKorrespondansepart>>,
    pub kildesystem: Option<String>,
    pub opprettet_av_command_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub dokumenter: Vec<AdminDokument>,
}

/// `opprettelse_`-prefikset er bevisst: feltene er input til `OpprettSak`,
/// ikke den nåværende saksansvarlige.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSakFakta {
    pub tilstand: String,
    pub sakstittel: Option<String>,
    pub arkivdel: Option<String>,
    pub ordningsverdi: Option<String>,
    pub opprettelse_saksbehandler_id: Option<String>,
    pub opprettelse_saksbehandler_enhet: Option<String>,
    pub tilgangskode: Option<String>,
    pub tilgangshjemmel: Option<String>,
    pub oensket_saksansvarlig_id: Option<String>,
    pub oensket_saksansvarlig_enhet: Option<String>,
    pub naavaerende_saksansvarlig_id: Option<String>,
    pub naavaerende_saksansvarlig_enhet: Option<String>,
    pub opprettet_av_command_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub journalposter: Vec<AdminJournalpost>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminOperasjonSammendrag {
    pub operasjon_id: OperasjonId,
    pub command_id: Uuid,
    pub operasjonstype: String,
    pub entitet_id: EntitetId,
    pub status: String,
}

/// `fakta` er `None` når identitet er mintet, men `sak_tilstand` ennå ikke
/// materialisert. Det er reparasjonsinformasjon, ikke en manglende sak.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSak {
    pub identitet: AdminEntitetIdentitet,
    pub fakta: Option<AdminSakFakta>,
    pub operasjoner: Vec<AdminOperasjonSammendrag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminSakNokkel {
    SkuffenId(SkuffenSakId),
    ClientReference(Uuid),
    ArkivId(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tidspunkt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-27T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn operasjon(status: &str) -> AdminOperasjonDetaljer {
        let entitet_id = EntitetId::Sak(SkuffenSakId(Uuid::new_v4()));
        AdminOperasjonDetaljer {
            operasjon_id: OperasjonId(Uuid::new_v4()),
            operasjonstype: "opprett_sak".to_string(),
            entitet: AdminOperasjonEntitet {
                skuffen_id: entitet_id,
                client_reference: None,
                arkiv_id: None,
            },
            sak_id: SkuffenSakId(entitet_id.as_uuid()),
            status: status.to_string(),
            attempt_no: 0,
            neste_forsok_at: None,
            blokkert_av: None,
            siste_detalj: None,
            sendt_at: None,
            ferdig_at: None,
            varslet_at: None,
            created_at: tidspunkt(),
            updated_at: tidspunkt(),
        }
    }

    fn command(statuser: &[&str]) -> AdminCommand {
        AdminCommand {
            command_id: Uuid::new_v4(),
            correlation_id: None,
            command_type: "opprett_sak".to_string(),
            mottatt_at: tidspunkt(),
            dispatchet_at: None,
            dekomponert_at: None,
            operasjoner: statuser.iter().map(|status| operasjon(status)).collect(),
        }
    }

    #[test]
    fn tom_operasjonsliste_er_uavklart_ikke_fullfort() {
        assert_eq!(command(&[]).utled_utfall(), AdminCommandUtfall::Uavklart);
    }

    #[test]
    fn paagaaende_operasjoner_er_uavklart() {
        assert_eq!(
            command(&["ok", "klar"]).utled_utfall(),
            AdminCommandUtfall::Uavklart
        );
    }

    #[test]
    fn alle_ok_er_fullfort() {
        assert_eq!(
            command(&["ok", "ok"]).utled_utfall(),
            AdminCommandUtfall::Fullfort
        );
    }

    #[test]
    fn krever_avklaring_skjules_ikke_som_uavklart() {
        assert_eq!(
            command(&["ok", "krever_avklaring"]).utled_utfall(),
            AdminCommandUtfall::KreverAvklaring
        );
    }

    #[test]
    fn feilet_har_prioritet_over_krever_avklaring() {
        assert_eq!(
            command(&["krever_avklaring", "feilet"]).utled_utfall(),
            AdminCommandUtfall::Feilet
        );
        assert_eq!(
            command(&["feilet", "ok"]).utled_utfall(),
            AdminCommandUtfall::Feilet
        );
    }
}
