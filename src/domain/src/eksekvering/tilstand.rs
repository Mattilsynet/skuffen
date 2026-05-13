use crate::eksekvering::html_template::{er_felter_klare, FeltVerdier};
use crate::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use crate::eksekvering::typer::{CommandTypeCode, EksekveringFeil};
use lib_schemas::skuffen::dokument::Felt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostType {
    Inngaende,
    Utgaaende,
    InterntNotat,
}

/// Saksansvarlig (Noark 5 M306) — identifiserer ansvarlig saksbehandler og enhet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saksansvarlig {
    pub saksbehandler_id: String,
    pub enhet: String,
}

// ---------------------------------------------------------------------------
// Tilstander
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SakTilstand {
    IkkeRealisert,
    Opprettet,
    Avsluttet,
    FeiletPermanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostTilstand {
    IkkeRealisert,
    Opprettet,
    DokumenterUnderArbeid,
    KlarForJournalforing,
    VenterPaaUtsending,
    Journalfoert,
    Avskrevet,
    FeiletPermanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DokumentTilstand {
    IkkeRealisert,
    AvventerRendring,
    Ok,
    FeiletPermanent,
}

// ---------------------------------------------------------------------------
// Aggregat-snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SakMedBarn {
    pub sak_id: SkuffenSakId,
    pub tilstand: SakTilstand,
    pub oensket_tilstand: SakTilstand,
    pub sikri_id: Option<i64>,
    pub saksnummer: Option<String>,
    /// Ønsket saksansvarlig (Noark 5 M306).
    /// Set when a SettSaksansvarlig command is registered.
    pub oensket_saksansvarlig: Option<Saksansvarlig>,
    /// Nåværende saksansvarlig satt i Sikri.
    /// Updated after successful Sikri call.
    pub naavaerende_saksansvarlig: Option<Saksansvarlig>,
    pub journalposter: Vec<JournalpostMedDokumenter>,
}

#[derive(Debug, Clone)]
pub struct JournalpostMedDokumenter {
    pub journalpost_id: SkuffenJournalpostId,
    pub tilstand: JournalpostTilstand,
    pub oensket_tilstand: JournalpostTilstand,
    pub sikri_id: Option<i64>,
    pub journalpostnummer: Option<i32>,
    pub journalposttype: JournalpostType,
    pub med_utsending: bool,
    pub dokumenter: Vec<DokumentMedTilstand>,
}

#[derive(Debug, Clone)]
pub struct DokumentMedTilstand {
    pub dokument_id: SkuffenDokumentId,
    pub tilstand: DokumentTilstand,
    pub kilde: DokumentKildeTilstand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DokumentKildeTilstand {
    Bytes,
    HtmlTemplate {
        mal_referanse: uuid::Uuid,
        felter: Vec<Felt>,
        rendered_dokument_referanse: Option<uuid::Uuid>,
    },
}

// ---------------------------------------------------------------------------
// Operasjoner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArkivOperasjon {
    OpprettSak {
        sak_id: SkuffenSakId,
    },
    OpprettJournalpost {
        journalpost_id: SkuffenJournalpostId,
    },
    LeggTilDokument {
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    },
    RenderDokument {
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    },
    Journalfoer {
        journalpost_id: SkuffenJournalpostId,
    },
    Avskriv {
        journalpost_id: SkuffenJournalpostId,
    },
    AvsluttSak {
        sak_id: SkuffenSakId,
    },
    SettSaksansvarlig {
        sak_id: SkuffenSakId,
    },
}

// ---------------------------------------------------------------------------
// Hjelpefunksjoner
// ---------------------------------------------------------------------------

pub fn oensket_sluttilstand_for_dokument() -> DokumentTilstand {
    DokumentTilstand::Ok
}

pub fn oensket_sluttilstand_for_journalpost(
    journalposttype: JournalpostType,
) -> JournalpostTilstand {
    match journalposttype {
        JournalpostType::Inngaende => JournalpostTilstand::Avskrevet,
        JournalpostType::Utgaaende | JournalpostType::InterntNotat => {
            JournalpostTilstand::Journalfoert
        }
    }
}

/// Returnerer true hvis alle entiteter har nådd sin ønskede tilstand.
pub fn er_ferdig(sak: &SakMedBarn) -> bool {
    if sak.tilstand != sak.oensket_tilstand {
        return false;
    }
    // Saksansvarlig (Noark 5 M306) must match if requested
    if sak.oensket_saksansvarlig != sak.naavaerende_saksansvarlig {
        return false;
    }
    sak.journalposter.iter().all(|jp| {
        jp.tilstand == jp.oensket_tilstand
            && jp
                .dokumenter
                .iter()
                .all(|d| d.tilstand == DokumentTilstand::Ok)
    })
}

// ---------------------------------------------------------------------------
// Tilstandsmaskin: neste handling
// ---------------------------------------------------------------------------

/// Gitt nåværende tilstander for saken og dens barn,
/// returnerer neste Sikri-operasjon som kan utføres, eller `None`
/// hvis alt er ferdig eller blokkert.
///
/// `command_type` er reservert for fremtidig bruk der kommandotypen
/// påvirker hvilke overganger som er tillatt (f.eks. AvsluttSak
/// trenger tilgang til hele sakens barn).
pub fn neste_handling(
    _command_type: CommandTypeCode,
    sak: &SakMedBarn,
) -> Result<Option<ArkivOperasjon>, EksekveringFeil> {
    // 1. Sak ikke realisert, men ønsket fremover
    if sak.tilstand == SakTilstand::IkkeRealisert
        && sak.oensket_tilstand != SakTilstand::IkkeRealisert
    {
        return Ok(Some(ArkivOperasjon::OpprettSak { sak_id: sak.sak_id }));
    }

    // 1b. Saksansvarlig (Noark 5 M306) ønsket men ikke satt — sett før journalposter
    //     Krever saksnummer (som step 2) for å unngå blocked-retry-syklus.
    if sak.tilstand == SakTilstand::Opprettet
        && sak.saksnummer.is_some()
        && sak.oensket_saksansvarlig.is_some()
        && sak.oensket_saksansvarlig != sak.naavaerende_saksansvarlig
    {
        return Ok(Some(ArkivOperasjon::SettSaksansvarlig {
            sak_id: sak.sak_id,
        }));
    }

    // 2. Journalpost ikke realisert, sak opprettet med saksnummer
    if sak.tilstand == SakTilstand::Opprettet && sak.saksnummer.is_some() {
        for jp in &sak.journalposter {
            if jp.tilstand == JournalpostTilstand::IkkeRealisert {
                return Ok(Some(ArkivOperasjon::OpprettJournalpost {
                    journalpost_id: jp.journalpost_id,
                }));
            }
        }
    }

    // 3. Dokument ikke realisert, journalpost i arbeidsfase
    for jp in &sak.journalposter {
        if matches!(
            jp.tilstand,
            JournalpostTilstand::Opprettet | JournalpostTilstand::DokumenterUnderArbeid
        ) {
            for dok in &jp.dokumenter {
                if dok.tilstand == DokumentTilstand::IkkeRealisert {
                    return Ok(Some(ArkivOperasjon::LeggTilDokument {
                        journalpost_id: jp.journalpost_id,
                        dokument_id: dok.dokument_id,
                    }));
                }
            }
        }
    }

    // 4. Feilet dokument — kan aldri hentes, avbryt ugjenkallelig
    for jp in &sak.journalposter {
        if jp
            .dokumenter
            .iter()
            .any(|d| d.tilstand == DokumentTilstand::FeiletPermanent)
        {
            return Err(EksekveringFeil::irrecoverable(format!(
                "Journalpost {} har dokument med permanent feil",
                jp.journalpost_id.0,
            )));
        }
    }

    // 4b. HTML-template dokument klart for rendering etter at felter finnes.
    for jp in &sak.journalposter {
        if matches!(
            jp.tilstand,
            JournalpostTilstand::Opprettet | JournalpostTilstand::DokumenterUnderArbeid
        ) {
            for dok in &jp.dokumenter {
                if dok.tilstand == DokumentTilstand::AvventerRendring
                    && dokument_kan_rendres(dok, sak.saksnummer.as_deref())
                {
                    return Ok(Some(ArkivOperasjon::RenderDokument {
                        journalpost_id: jp.journalpost_id,
                        dokument_id: dok.dokument_id,
                    }));
                }
            }
        }
    }

    // 5. Alle dokumenter Ok → journalfør
    for jp in &sak.journalposter {
        if matches!(
            jp.tilstand,
            JournalpostTilstand::Opprettet
                | JournalpostTilstand::DokumenterUnderArbeid
                | JournalpostTilstand::KlarForJournalforing
        ) {
            let alle_dok_ok = !jp.dokumenter.is_empty()
                && jp
                    .dokumenter
                    .iter()
                    .all(|d| d.tilstand == DokumentTilstand::Ok);
            if alle_dok_ok {
                return Ok(Some(ArkivOperasjon::Journalfoer {
                    journalpost_id: jp.journalpost_id,
                }));
            }
        }
    }

    // 6. Journalført → avskriv (kun inngående)
    for jp in &sak.journalposter {
        if jp.tilstand == JournalpostTilstand::Journalfoert
            && jp.oensket_tilstand == JournalpostTilstand::Avskrevet
        {
            return Ok(Some(ArkivOperasjon::Avskriv {
                journalpost_id: jp.journalpost_id,
            }));
        }
    }

    // 7. Journalført med ønsket Journalført → ferdig, skip.
    //    VenterPaaUtsending → blokkert på ekstern utsending, skip.

    // 8. Avslutt sak hvis ønsket og alle journalposter terminale
    //    (Noark 5 6.1.13: saksansvarlig must be correct before avslutting)
    if sak.oensket_tilstand == SakTilstand::Avsluttet && sak.tilstand == SakTilstand::Opprettet {
        // Guard: saksansvarlig must match (or not be requested at all)
        if sak.oensket_saksansvarlig != sak.naavaerende_saksansvarlig {
            return Ok(Some(ArkivOperasjon::SettSaksansvarlig {
                sak_id: sak.sak_id,
            }));
        }

        if sak.journalposter.is_empty() {
            return Ok(Some(ArkivOperasjon::AvsluttSak { sak_id: sak.sak_id }));
        }

        let alle_terminale = sak.journalposter.iter().all(er_terminal_journalpost);
        if alle_terminale {
            return Ok(Some(ArkivOperasjon::AvsluttSak { sak_id: sak.sak_id }));
        }
    }

    // 9. Ingenting å gjøre
    Ok(None)
}

fn er_terminal_journalpost(jp: &JournalpostMedDokumenter) -> bool {
    match jp.journalposttype {
        JournalpostType::Inngaende => jp.tilstand == JournalpostTilstand::Avskrevet,
        JournalpostType::Utgaaende => {
            if jp.med_utsending {
                jp.tilstand == JournalpostTilstand::VenterPaaUtsending
            } else {
                jp.tilstand == JournalpostTilstand::Journalfoert
            }
        }
        JournalpostType::InterntNotat => jp.tilstand == JournalpostTilstand::Journalfoert,
    }
}

fn dokument_kan_rendres(dok: &DokumentMedTilstand, saksnummer: Option<&str>) -> bool {
    match &dok.kilde {
        DokumentKildeTilstand::HtmlTemplate {
            felter,
            rendered_dokument_referanse,
            mal_referanse: _,
        } => {
            rendered_dokument_referanse.is_none()
                && er_felter_klare(felter, &FeltVerdier { saksnummer })
        }
        DokumentKildeTilstand::Bytes => false,
    }
}

// ---------------------------------------------------------------------------
// Tester
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sak_id() -> SkuffenSakId {
        SkuffenSakId(Uuid::new_v4())
    }

    fn jp_id() -> SkuffenJournalpostId {
        SkuffenJournalpostId(Uuid::new_v4())
    }

    fn dok_id() -> SkuffenDokumentId {
        SkuffenDokumentId(Uuid::new_v4())
    }

    fn enkel_sak(tilstand: SakTilstand, oensket: SakTilstand) -> SakMedBarn {
        SakMedBarn {
            sak_id: sak_id(),
            tilstand,
            oensket_tilstand: oensket,
            sikri_id: None,
            saksnummer: None,
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![],
        }
    }

    fn opprettet_sak_med_saksnummer(journalposter: Vec<JournalpostMedDokumenter>) -> SakMedBarn {
        SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::Opprettet,
            oensket_tilstand: SakTilstand::Opprettet,
            sikri_id: Some(1),
            saksnummer: Some("2025/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter,
        }
    }

    fn lag_journalpost(
        tilstand: JournalpostTilstand,
        oensket: JournalpostTilstand,
        jptype: JournalpostType,
        med_utsending: bool,
        dokumenter: Vec<DokumentMedTilstand>,
    ) -> JournalpostMedDokumenter {
        JournalpostMedDokumenter {
            journalpost_id: jp_id(),
            tilstand,
            oensket_tilstand: oensket,
            sikri_id: Some(100),
            journalpostnummer: Some(1),
            journalposttype: jptype,
            med_utsending,
            dokumenter,
        }
    }

    fn dok(tilstand: DokumentTilstand) -> DokumentMedTilstand {
        DokumentMedTilstand {
            dokument_id: dok_id(),
            tilstand,
            kilde: DokumentKildeTilstand::Bytes,
        }
    }

    fn template_dok(tilstand: DokumentTilstand, felter: Vec<Felt>) -> DokumentMedTilstand {
        DokumentMedTilstand {
            dokument_id: dok_id(),
            tilstand,
            kilde: DokumentKildeTilstand::HtmlTemplate {
                mal_referanse: uuid::Uuid::new_v4(),
                felter,
                rendered_dokument_referanse: None,
            },
        }
    }

    // 1. IkkeRealisert sak med ønsket Opprettet → OpprettSak
    #[test]
    fn sak_uten_barn_ikke_realisert_gir_opprett_sak() {
        let sak = enkel_sak(SakTilstand::IkkeRealisert, SakTilstand::Opprettet);
        let resultat = neste_handling(CommandTypeCode::OpprettSak, &sak).unwrap();
        assert!(matches!(resultat, Some(ArkivOperasjon::OpprettSak { .. })));
    }

    // 2. Opprettet sak uten barn er ferdig
    #[test]
    fn sak_opprettet_uten_barn_er_ferdig() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        let resultat = neste_handling(CommandTypeCode::OpprettSak, &sak).unwrap();
        assert_eq!(resultat, None);
    }

    // 3. Full livssyklus for inngående journalpost
    #[test]
    fn journalpost_ordering_inngaende() {
        let d = dok(DokumentTilstand::IkkeRealisert);
        let jp = lag_journalpost(
            JournalpostTilstand::IkkeRealisert,
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![d],
        );

        // Steg 1: sak ikke realisert → opprett sak
        let mut sak = SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::IkkeRealisert,
            oensket_tilstand: SakTilstand::Opprettet,
            sikri_id: None,
            saksnummer: None,
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![jp],
        };
        let r = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert!(matches!(r, Some(ArkivOperasjon::OpprettSak { .. })));

        // Steg 2: sak opprettet → opprett journalpost
        sak.tilstand = SakTilstand::Opprettet;
        sak.sikri_id = Some(1);
        sak.saksnummer = Some("2025/1".to_string());
        let r = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert!(matches!(r, Some(ArkivOperasjon::OpprettJournalpost { .. })));

        // Steg 3: journalpost opprettet → legg til dokument
        sak.journalposter[0].tilstand = JournalpostTilstand::Opprettet;
        let r = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert!(matches!(r, Some(ArkivOperasjon::LeggTilDokument { .. })));

        // Steg 4: dokument ok → journalfør
        sak.journalposter[0].dokumenter[0].tilstand = DokumentTilstand::Ok;
        let r = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert!(matches!(r, Some(ArkivOperasjon::Journalfoer { .. })));

        // Steg 5: journalført → avskriv
        sak.journalposter[0].tilstand = JournalpostTilstand::Journalfoert;
        let r = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert!(matches!(r, Some(ArkivOperasjon::Avskriv { .. })));

        // Steg 6: avskrevet → ferdig
        sak.journalposter[0].tilstand = JournalpostTilstand::Avskrevet;
        let r = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert_eq!(r, None);
    }

    // 4. Feilet dokument gir irrecoverable feil (ikke blokkert — vil aldri bli Ok)
    #[test]
    fn feilet_dokument_gir_irrecoverable() {
        let jp = lag_journalpost(
            JournalpostTilstand::DokumenterUnderArbeid,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![
                dok(DokumentTilstand::Ok),
                dok(DokumentTilstand::FeiletPermanent),
            ],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);
        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak);
        assert!(resultat.is_err());
        assert_eq!(
            resultat.unwrap_err().feiltype,
            crate::eksekvering::typer::EksekveringFeiltype::Irrecoverable
        );
    }

    #[test]
    fn permanent_feil_vinner_over_rendering() {
        let jp = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![
                template_dok(DokumentTilstand::AvventerRendring, vec![Felt::Saksnummer]),
                dok(DokumentTilstand::FeiletPermanent),
            ],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);
        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak);
        assert!(resultat.is_err());
    }

    #[test]
    fn html_template_venter_paa_saksnummer() {
        let jp = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.saksnummer = None;

        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak).unwrap();

        assert_eq!(resultat, None);
    }

    #[test]
    fn html_template_rendres_for_journalfoering() {
        let jp = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak).unwrap();

        assert!(matches!(
            resultat,
            Some(ArkivOperasjon::RenderDokument { .. })
        ));
    }

    #[test]
    fn journalfoering_venter_paa_rendered_hoveddokument() {
        let jp = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak).unwrap();

        assert!(!matches!(
            resultat,
            Some(ArkivOperasjon::Journalfoer { .. })
        ));
    }

    // 5. Utgående journalpost som er Journalfoert med ønsket Journalfoert → None
    #[test]
    fn avskriving_kun_for_inngaende() {
        let jp = lag_journalpost(
            JournalpostTilstand::Journalfoert,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);
        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak).unwrap();
        assert_eq!(resultat, None);
    }

    // 6. AvsluttSak blokkert av uferdige journalposter
    #[test]
    fn avslutt_sak_blokkert_av_uferdige_journalposter() {
        let jp = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Journalfoert,
            JournalpostType::InterntNotat,
            false,
            vec![dok(DokumentTilstand::IkkeRealisert)],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.oensket_tilstand = SakTilstand::Avsluttet;
        // Journalpost is not terminal, but no FeiletPermanent docs either → None
        // (the LeggTilDokument step fires first since doc is IkkeRealisert and jp is Opprettet)
        let resultat = neste_handling(CommandTypeCode::AvsluttSak, &sak).unwrap();
        assert!(matches!(
            resultat,
            Some(ArkivOperasjon::LeggTilDokument { .. })
        ));
    }

    // 7. Utgående med utsending, VenterPaaUtsending → None
    #[test]
    fn venter_paa_utsending_for_utgaaende() {
        let jp = lag_journalpost(
            JournalpostTilstand::VenterPaaUtsending,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            true,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);
        let resultat = neste_handling(CommandTypeCode::OpprettUtgaaendeJournalpost, &sak).unwrap();
        assert_eq!(resultat, None);
    }

    // 8. Alt ferdig → None
    #[test]
    fn alt_ferdig_gir_none() {
        let jp = lag_journalpost(
            JournalpostTilstand::Avskrevet,
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);
        let resultat = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        assert_eq!(resultat, None);
    }

    // 9. er_ferdig fungerer
    #[test]
    fn er_ferdig_fungerer() {
        let jp = lag_journalpost(
            JournalpostTilstand::Avskrevet,
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);
        assert!(er_ferdig(&sak));

        let jp2 = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::IkkeRealisert)],
        );
        let sak2 = opprettet_sak_med_saksnummer(vec![jp2]);
        assert!(!er_ferdig(&sak2));
    }

    // 10. AvsluttSak når alle journalposter er terminale
    #[test]
    fn avslutt_sak_naar_alle_journalposter_terminale() {
        let jp_inn = lag_journalpost(
            JournalpostTilstand::Avskrevet,
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let jp_ut = lag_journalpost(
            JournalpostTilstand::Journalfoert,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp_inn, jp_ut]);
        sak.oensket_tilstand = SakTilstand::Avsluttet;
        let resultat = neste_handling(CommandTypeCode::AvsluttSak, &sak).unwrap();
        assert!(matches!(resultat, Some(ArkivOperasjon::AvsluttSak { .. })));
    }

    // 11. AvsluttSak med tom journalposter-vektor
    #[test]
    fn avslutt_sak_uten_journalposter() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Avsluttet);
        sak.saksnummer = Some("2025/1".to_string());
        let resultat = neste_handling(CommandTypeCode::AvsluttSak, &sak).unwrap();
        assert!(matches!(resultat, Some(ArkivOperasjon::AvsluttSak { .. })));
    }

    // 12. Flere journalposter på forskjellige stadier
    #[test]
    fn flere_journalposter_forskjellige_stadier() {
        let jp_ferdig = lag_journalpost(
            JournalpostTilstand::Journalfoert,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let jp_under_arbeid = lag_journalpost(
            JournalpostTilstand::DokumenterUnderArbeid,
            JournalpostTilstand::Journalfoert,
            JournalpostType::InterntNotat,
            false,
            vec![
                dok(DokumentTilstand::Ok),
                dok(DokumentTilstand::IkkeRealisert),
            ],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp_ferdig, jp_under_arbeid]);
        let resultat =
            neste_handling(CommandTypeCode::OpprettInterntNotatJournalpost, &sak).unwrap();
        // Bør plukke opp det urealiserte dokumentet på jp_under_arbeid
        assert!(matches!(
            resultat,
            Some(ArkivOperasjon::LeggTilDokument { .. })
        ));
    }

    // 13. FeiletPermanent journalpost blokkerer AvsluttSak
    #[test]
    fn feilet_permanent_journalpost_blokkerer_avslutt_sak() {
        let jp = lag_journalpost(
            JournalpostTilstand::Opprettet,
            JournalpostTilstand::Journalfoert,
            JournalpostType::InterntNotat,
            false,
            vec![
                dok(DokumentTilstand::Ok),
                dok(DokumentTilstand::FeiletPermanent),
            ],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.oensket_tilstand = SakTilstand::Avsluttet;
        let resultat = neste_handling(CommandTypeCode::AvsluttSak, &sak);
        assert!(resultat.is_err());
    }

    // -----------------------------------------------------------------------
    // SettSaksansvarlig (Noark 5 M306) tests
    // -----------------------------------------------------------------------

    fn saksansvarlig(id: &str, enhet: &str) -> Option<Saksansvarlig> {
        Some(Saksansvarlig {
            saksbehandler_id: id.to_string(),
            enhet: enhet.to_string(),
        })
    }

    // 14. SettSaksansvarlig fires when ønsket != nåværende
    #[test]
    fn sett_saksansvarlig_naar_oensket_ulik_naavaerende() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = saksansvarlig("Z12345", "42");
        sak.naavaerende_saksansvarlig = None;
        let resultat = neste_handling(CommandTypeCode::SettSaksansvarlig, &sak).unwrap();
        assert!(matches!(
            resultat,
            Some(ArkivOperasjon::SettSaksansvarlig { .. })
        ));
    }

    // 15. SettSaksansvarlig does NOT fire when ønsket == nåværende (idempotent)
    #[test]
    fn sett_saksansvarlig_idempotent_naar_lik() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = saksansvarlig("Z12345", "42");
        sak.naavaerende_saksansvarlig = saksansvarlig("Z12345", "42");
        let resultat = neste_handling(CommandTypeCode::SettSaksansvarlig, &sak).unwrap();
        assert_eq!(resultat, None);
    }

    // 16. SettSaksansvarlig does NOT fire when ønsket is None
    #[test]
    fn sett_saksansvarlig_ikke_naar_oensket_er_none() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = None;
        sak.naavaerende_saksansvarlig = None;
        let resultat = neste_handling(CommandTypeCode::SettSaksansvarlig, &sak).unwrap();
        assert_eq!(resultat, None);
    }

    // 17. SettSaksansvarlig fires before journalpost work
    #[test]
    fn sett_saksansvarlig_foer_journalpost() {
        let jp = lag_journalpost(
            JournalpostTilstand::IkkeRealisert,
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::IkkeRealisert)],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.oensket_saksansvarlig = saksansvarlig("Z12345", "42");
        sak.naavaerende_saksansvarlig = None;
        let resultat = neste_handling(CommandTypeCode::OpprettInngaaendeJournalpost, &sak).unwrap();
        // Should pick sett_saksansvarlig BEFORE opprett_journalpost
        assert!(matches!(
            resultat,
            Some(ArkivOperasjon::SettSaksansvarlig { .. })
        ));
    }

    // 18. AvsluttSak blocked when saksansvarlig not yet set
    #[test]
    fn avslutt_sak_blokkert_naar_saksansvarlig_ikke_satt() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Avsluttet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = saksansvarlig("Z12345", "42");
        sak.naavaerende_saksansvarlig = None;
        let resultat = neste_handling(CommandTypeCode::AvsluttSak, &sak).unwrap();
        // Should return SettSaksansvarlig instead of AvsluttSak
        assert!(matches!(
            resultat,
            Some(ArkivOperasjon::SettSaksansvarlig { .. })
        ));
    }

    // 19. AvsluttSak proceeds when saksansvarlig matches
    #[test]
    fn avslutt_sak_naar_saksansvarlig_satt() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Avsluttet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = saksansvarlig("Z12345", "42");
        sak.naavaerende_saksansvarlig = saksansvarlig("Z12345", "42");
        let resultat = neste_handling(CommandTypeCode::AvsluttSak, &sak).unwrap();
        assert!(matches!(resultat, Some(ArkivOperasjon::AvsluttSak { .. })));
    }

    // 20. er_ferdig respects saksansvarlig mismatch
    #[test]
    fn er_ferdig_sjekker_saksansvarlig() {
        let mut sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Opprettet);
        sak.oensket_saksansvarlig = saksansvarlig("Z12345", "42");
        sak.naavaerende_saksansvarlig = None;
        assert!(!er_ferdig(&sak));

        sak.naavaerende_saksansvarlig = saksansvarlig("Z12345", "42");
        assert!(er_ferdig(&sak));
    }

    // 21. er_ferdig ok when no saksansvarlig requested
    #[test]
    fn er_ferdig_ok_uten_saksansvarlig() {
        let sak = enkel_sak(SakTilstand::Opprettet, SakTilstand::Opprettet);
        assert!(er_ferdig(&sak));
    }
}
