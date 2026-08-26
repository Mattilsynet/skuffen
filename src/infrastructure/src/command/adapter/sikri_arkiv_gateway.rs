use crate::command::media::MediaStore;
use application::command::materialisering::{
    DokumentAttributter, Dokumentkilde, JournalpostAttributter, Korrespondanseparter,
    SakAttributter, Tilgang,
};
use application::command::ports::eksekvering_port::{
    ArkivGateway, Journalstatus, ObservertJournalstatus, OpprettJournalpostResultat,
    OpprettSakResultat,
};
use application::command::{
    Arkivdel, Korrespondansepart, MottakerId, Parttype, Utsendingsmottaker,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::typer::{EksekveringFeil, StatusErrorCode};
use sikri_client::domain::ny_sak::NySak;
use sikri_client::dto::elements_avsender_mottaker::ElementsAvsenderMottaker;
use sikri_client::dto::elements_dokument::ElementsDokument;
use sikri_client::dto::elements_journalpost::ElementsJournalpost;
use sikri_client::{Recoverability, SikriFeil};

/// Leverandørvokabularet stopper her.
///
/// `SikriFeil` bærer allerede klassifisering, stabil kode og en trygg,
/// ferdigmappet brukertekst. Denne funksjonen legger kun til hvilken
/// klientvendt feilkode koden svarer til — det er den ene oversettelsen
/// `sikri_client` ikke kan gjøre selv, siden `StatusErrorCode` bor i `domain`.
fn fra_sikri(feil: SikriFeil) -> EksekveringFeil {
    let error_code = error_code_for(feil.kode).unwrap_or(StatusErrorCode::ProcessingFailed);
    match feil.recoverability {
        Recoverability::Recoverable => {
            EksekveringFeil::recoverable(feil.kode, feil.melding, error_code)
        }
        Recoverability::Irrecoverable => {
            EksekveringFeil::irrecoverable(feil.kode, feil.melding, error_code)
        }
    }
}

/// `None` betyr at koden ikke er tatt stilling til. Kalleren faller til
/// `ProcessingFailed`, og dekningstesten nederst fanger hullet.
fn error_code_for(kode: &str) -> Option<StatusErrorCode> {
    let error_code = match kode {
        "sikri_unknown_user"
        | "sikri_access_control_rejected"
        | "sikri_validation_failed"
        | "sikri_missing_document_content"
        | "sikri_invalid_request"
        | "sikri_request_validation_failed" => StatusErrorCode::InvalidRequest,
        "sikri_resource_not_found" => StatusErrorCode::NotFound,
        "sikri_rate_limited"
        | "sikri_upstream_unavailable"
        | "sikri_upstream_error"
        | "sikri_secret_unavailable" => StatusErrorCode::TemporaryUnavailable,
        "sikri_response_unparsable" | "sikri_unknown_error" => StatusErrorCode::ProcessingFailed,
        _ => return None,
    };
    Some(error_code)
}

/// Våre egne mappingfeil. Ekte irrecoverable — samme payload gir samme feil
/// hver gang — og `client_reference` peker på nøyaktig hvilket dokument eller
/// hvilken korrespondansepart klienten må rette.
fn arkivmapping_feil(
    kode: &'static str,
    melding: &str,
    client_reference: uuid::Uuid,
) -> EksekveringFeil {
    EksekveringFeil::irrecoverable(
        kode,
        format!("{melding} (client_reference={client_reference})"),
        StatusErrorCode::InvalidRequest,
    )
}

#[derive(Clone)]
pub struct SikriArkivGateway {
    media_store: std::sync::Arc<dyn MediaStore>,
}

impl SikriArkivGateway {
    pub fn new(media_store: std::sync::Arc<dyn MediaStore>) -> Self {
        Self { media_store }
    }
}

#[async_trait]
impl ArkivGateway for SikriArkivGateway {
    async fn opprett_sak(
        &self,
        attributter: &SakAttributter,
    ) -> Result<OpprettSakResultat, EksekveringFeil> {
        let ny_sak = NySak {
            sakstittel: attributter.sakstittel.clone(),
            arkivdel: match attributter.arkivdel {
                Arkivdel::Tilsynsdivisjonene => {
                    sikri_client::domain::ny_sak::Arkivdel::Tilsynsdivisjonene
                }
                Arkivdel::Hovedkontoret => sikri_client::domain::ny_sak::Arkivdel::Hovedkontoret,
            },
            saksbehandler_id: attributter.saksbehandler_id.clone(),
            saksbehandler_enhet: attributter.saksbehandler_enhet.clone(),
            ordningsverdi: attributter.ordningsverdi.clone(),
            tilgang: match (
                attributter.tilgang.tilgangskode.as_ref(),
                attributter.tilgang.tilgangshjemmel.as_ref(),
            ) {
                (Some(tilgangskode), Some(tilgangshjemmel)) => {
                    Some(sikri_client::domain::ny_sak::Tilgang {
                        tilgangskode: tilgangskode.clone(),
                        tilgangshjemmel: tilgangshjemmel.clone(),
                    })
                }
                _ => None,
            },
            virksomhetsmappe_id: None,
        };

        let resp = sikri_client::opprett_sak(ny_sak).await.map_err(fra_sikri)?;
        let saksnummer = resp.saksnr.ok_or_else(|| {
            EksekveringFeil::recoverable(
                "sikri_response_unparsable",
                "Uventet svar fra Sikri/Elements. Prøv igjen senere.",
                StatusErrorCode::TemporaryUnavailable,
            )
        })?;
        Ok(OpprettSakResultat { saksnummer })
    }

    async fn opprett_journalpost(
        &self,
        saksnummer: &str,
        journalpost: &JournalpostAttributter,
        hoveddokument: &DokumentAttributter,
    ) -> Result<OpprettJournalpostResultat, EksekveringFeil> {
        let elements_journalpost = self.map_journalpost(journalpost, hoveddokument).await?;
        let resp = sikri_client::opprett_journalpost(elements_journalpost, saksnummer)
            .await
            .map_err(fra_sikri)?;
        let journalpost_id = resp.journalpost_id.ok_or_else(|| {
            EksekveringFeil::recoverable(
                "sikri_response_unparsable",
                "Uventet svar fra Sikri/Elements. Prøv igjen senere.",
                StatusErrorCode::TemporaryUnavailable,
            )
        })?;
        Ok(OpprettJournalpostResultat { journalpost_id })
    }

    /// Ett vedlegg om gangen (D5). Sikris batch-API returnerer
    /// `Vec<Option<i32>>`, og partial success er ikke håndterbart i batch.
    async fn legg_til_vedlegg(
        &self,
        journalpost_id: i32,
        vedlegg: &DokumentAttributter,
    ) -> Result<Option<i32>, EksekveringFeil> {
        let dokument = self.map_dokument(vedlegg, false).await?;
        let resp = sikri_client::legg_til_vedlegg(journalpost_id, vec![dokument])
            .await
            .map_err(fra_sikri)?;
        Ok(resp.into_iter().next().and_then(|d| d.dokument_id))
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: Journalstatus,
    ) -> Result<(), EksekveringFeil> {
        sikri_client::sett_journalpost_status(journalpost_id, status.as_arkivkode())
            .await
            .map_err(fra_sikri)
    }

    /// Kun inngående avskrives (D21). `TE` — tatt til etterretning.
    async fn avskriv_journalpost(&self, journalpost_id: i32) -> Result<(), EksekveringFeil> {
        sikri_client::avskriv_journalpost(journalpost_id, "TE")
            .await
            .map_err(fra_sikri)
    }

    async fn hent_journalstatus(
        &self,
        journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, EksekveringFeil> {
        let journalpost = sikri_client::hent_journalpost(journalpost_id)
            .await
            .map_err(fra_sikri)?;
        Ok(match journalpost.journalstatus.as_deref() {
            Some("R") => ObservertJournalstatus::Reservert,
            Some("F") => ObservertJournalstatus::KlarForEkspedering,
            Some("E") => ObservertJournalstatus::Ekspedert,
            Some("J") => ObservertJournalstatus::Journalfoert,
            _ => ObservertJournalstatus::Annet,
        })
    }

    async fn avslutt_sak(&self, saksnummer: &str) -> Result<(), EksekveringFeil> {
        sikri_client::avslutt_sak(saksnummer)
            .await
            .map_err(fra_sikri)
    }

    async fn sett_saksansvarlig(
        &self,
        saksnummer: &str,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), EksekveringFeil> {
        sikri_client::sett_saksansvarlig(saksnummer, saksbehandler_id, saksbehandler_enhet)
            .await
            .map_err(fra_sikri)
    }
}

impl SikriArkivGateway {
    /// Bygger journalposten.
    ///
    /// Journalposter opprettes **aldri** direkte i `J` (SKU-0016 R10). For `I`
    /// og `X` settes `journalstatus` ikke i det hele tatt — Sikri åpner dem i
    /// en status der endringer er mulige, slik at vedlegg kan legges til
    /// etterpå. `avskrivDirekte` og `avskrivningsmaate` settes heller ikke ved
    /// opprettelse; avskriving er en egen operasjon.
    async fn map_journalpost(
        &self,
        journalpost: &JournalpostAttributter,
        hoveddokument: &DokumentAttributter,
    ) -> Result<ElementsJournalpost, EksekveringFeil> {
        let client_reference = journalpost.client_reference;
        let skjerming = skjerming_fra_tilgang(&journalpost.tilgang, client_reference)?;
        let dokumenter = vec![self.map_dokument(hoveddokument, true).await?];

        let journalstatus = match journalpost.journalposttype {
            // Utgående starter i R og flyttes videre av egne operasjoner.
            JournalpostType::Utgaaende => Some("R".to_string()),
            JournalpostType::Inngaende | JournalpostType::InterntNotat => None,
        };

        let avsendere_mottakere =
            self.map_korrespondanseparter(journalpost, &skjerming, client_reference)?;

        let elements = ElementsJournalpost {
            tittel: Some(journalpost.tittel.clone()),
            journalposttype: Some(journalpost.journalposttype.as_arkivkode().to_string()),
            journalstatus,
            avskriv_direkte: None,
            avskrivningsmaate: None,
            tilgangskode: skjerming.tilgangskode(),
            tilgangshjemmel: skjerming.tilgangshjemmel(),
            saksbehandler: Some(journalpost.saksbehandler_id.clone()),
            saksbehandler_enhet: Some(journalpost.saksbehandler_enhet.clone()),
            avsendere_mottakere,
            dokumenter: Some(dokumenter),
            dokument_dato: Some(journalpost.dokument_dato.clone()),
        };

        verifiser_skjerming(&elements, &skjerming, client_reference)?;
        Ok(elements)
    }

    fn map_korrespondanseparter(
        &self,
        journalpost: &JournalpostAttributter,
        skjerming: &Skjerming,
        client_reference: uuid::Uuid,
    ) -> Result<Option<Vec<ElementsAvsenderMottaker>>, EksekveringFeil> {
        // GENERELL trigger SvarUt; DIG brukes når utsending ikke benyttes.
        let forsendelsesmetode = if journalpost.med_utsending {
            "GENERELL"
        } else {
            "DIG"
        };

        let parter: Vec<ElementsAvsenderMottaker> = match &journalpost.korrespondanseparter {
            Korrespondanseparter::Ingen => return Ok(None),
            Korrespondanseparter::Avsender(avsender) => {
                vec![korrespondansepart_avsender_mottaker(
                    avsender, false, skjerming,
                )?]
            }
            Korrespondanseparter::Mottakere(mottakere) => mottakere
                .iter()
                .map(|mottaker| {
                    let mut am = korrespondansepart_avsender_mottaker(mottaker, true, skjerming)?;
                    am.forsendelsesmetode = Some(forsendelsesmetode.to_string());
                    Ok(am)
                })
                .collect::<Result<Vec<_>, EksekveringFeil>>()?,
            Korrespondanseparter::Utsendingsmottakere(mottakere) => mottakere
                .iter()
                .map(|mottaker| {
                    utsendingsmottaker_avsender_mottaker(mottaker, skjerming, client_reference)
                })
                .collect::<Result<Vec<_>, EksekveringFeil>>()?,
        };

        if parter.is_empty() {
            return Err(arkivmapping_feil(
                "arkivmapping_mottaker_mangler",
                "Mottaker mangler.",
                client_reference,
            ));
        }

        Ok(Some(parter))
    }

    async fn map_dokument(
        &self,
        dokument: &DokumentAttributter,
        hoveddokument: bool,
    ) -> Result<ElementsDokument, EksekveringFeil> {
        // For en mal er det den rendrede PDF-en som skal til arkivet; original
        // mal_referanse sendes aldri (SKU-0005).
        let (referanse, filtype) = match &dokument.kilde {
            Dokumentkilde::Bytes {
                dokument_referanse,
                filtype,
            } => (*dokument_referanse, filtype.clone()),
            Dokumentkilde::HtmlTemplate {
                rendered_dokument_referanse,
                ..
            } => {
                let referanse = rendered_dokument_referanse
                    .ok_or_else(|| EksekveringFeil::intern("arkivmapping_urendret_mal"))?;
                (referanse, "PDF".to_string())
            }
        };

        let innhold = self.hent_media_base64(referanse).await?;
        Ok(ElementsDokument {
            tittel: Some(dokument.tittel.clone()),
            hoveddokument,
            filtype: Some(filtype),
            innhold: Some(innhold),
        })
    }

    async fn hent_media_base64(
        &self,
        dokument_referanse: uuid::Uuid,
    ) -> Result<String, EksekveringFeil> {
        let media = self
            .media_store
            .get(dokument_referanse)
            .await
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_media_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| {
                EksekveringFeil::intern("intern_media_mangler")
                    .med_intern_detalj(format!("dokument_referanse={dokument_referanse}"))
            })?;
        Ok(STANDARD.encode(media.data))
    }
}

enum Skjerming {
    Offentlig,
    Skjermet {
        tilgangskode: String,
        tilgangshjemmel: String,
    },
}

impl Skjerming {
    fn er_skjermet(&self) -> bool {
        matches!(self, Skjerming::Skjermet { .. })
    }

    fn tilgangskode(&self) -> Option<String> {
        match self {
            Skjerming::Skjermet { tilgangskode, .. } => Some(tilgangskode.clone()),
            Skjerming::Offentlig => None,
        }
    }

    fn tilgangshjemmel(&self) -> Option<String> {
        match self {
            Skjerming::Skjermet {
                tilgangshjemmel, ..
            } => Some(tilgangshjemmel.clone()),
            Skjerming::Offentlig => None,
        }
    }
}

fn skjerming_fra_tilgang(
    tilgang: &Tilgang,
    client_reference: uuid::Uuid,
) -> Result<Skjerming, EksekveringFeil> {
    match (
        tilgang.tilgangskode.as_ref(),
        tilgang.tilgangshjemmel.as_ref(),
    ) {
        (None, None) => Ok(Skjerming::Offentlig),
        (Some(tilgangskode), Some(tilgangshjemmel)) => Ok(Skjerming::Skjermet {
            tilgangskode: tilgangskode.clone(),
            tilgangshjemmel: tilgangshjemmel.clone(),
        }),
        // Skjemet krever begge; halv skjerming er en mappingfeil, ikke noe å
        // gjette på (SKU-0015).
        _ => Err(arkivmapping_feil(
            "arkivmapping_ufullstendig_skjerming",
            "Ufullstendig skjerming: tilgangskode og tilgangshjemmel må settes sammen.",
            client_reference,
        )),
    }
}

fn unntatt_offentlighet(skjerming: &Skjerming) -> Option<bool> {
    Some(skjerming.er_skjermet())
}

fn person_flagg(parttype: Parttype) -> bool {
    match parttype {
        Parttype::Person => true,
        Parttype::Virksomhet => false,
    }
}

fn korrespondansepart_avsender_mottaker(
    part: &Korrespondansepart,
    er_mottaker: bool,
    skjerming: &Skjerming,
) -> Result<ElementsAvsenderMottaker, EksekveringFeil> {
    Ok(ElementsAvsenderMottaker {
        er_mottaker: Some(er_mottaker),
        navn: Some(part.navn.clone()),
        forsendelsesmetode: None,
        kopi: None,
        unntatt_offentlighet: unntatt_offentlighet(skjerming),
        person: Some(person_flagg(part.parttype)),
        til_saksbehandler: None,
        til_saksbehandler_enhet: None,
        id: None,
        organisasjonsnummer: None,
        epost: None,
        telefon: None,
        postadresse: None,
        postnummer: None,
        poststed: None,
        utlandsadresse: None,
    })
}

fn utsendingsmottaker_avsender_mottaker(
    mottaker: &Utsendingsmottaker,
    skjerming: &Skjerming,
    client_reference: uuid::Uuid,
) -> Result<ElementsAvsenderMottaker, EksekveringFeil> {
    // postnummer er en validert Postnummer-newtype (4 siffer), så kun de rå
    // String-feltene må sjekkes for tomhet her.
    if mottaker.adresse.adresse.trim().is_empty() || mottaker.adresse.poststed.trim().is_empty() {
        return Err(arkivmapping_feil(
            "arkivmapping_postadresse_mangler",
            "Postadresse mangler.",
            client_reference,
        ));
    }

    // Sikri gjenbruker organisasjonsnummer-feltet for både orgnr og fnr;
    // person-flagget skiller dem. Fnr skal derfor ikke droppes.
    let (person, organisasjonsnummer) = match &mottaker.id {
        MottakerId::Person { fødselsnummer } => {
            (Some(true), Some(fødselsnummer.as_str().to_string()))
        }
        MottakerId::Virksomhet {
            organisasjonsnummer,
        } => (Some(false), Some(organisasjonsnummer.as_str().to_string())),
    };

    Ok(ElementsAvsenderMottaker {
        er_mottaker: Some(true),
        navn: Some(mottaker.navn.clone()),
        forsendelsesmetode: Some("GENERELL".to_string()),
        kopi: None,
        unntatt_offentlighet: unntatt_offentlighet(skjerming),
        person,
        til_saksbehandler: None,
        til_saksbehandler_enhet: None,
        id: None,
        organisasjonsnummer,
        epost: None,
        telefon: None,
        postadresse: Some(mottaker.adresse.adresse.clone()),
        postnummer: Some(mottaker.adresse.postnummer.as_str().to_string()),
        poststed: Some(mottaker.adresse.poststed.clone()),
        utlandsadresse: None,
    })
}

fn verifiser_skjerming(
    journalpost: &ElementsJournalpost,
    skjerming: &Skjerming,
    client_reference: uuid::Uuid,
) -> Result<(), EksekveringFeil> {
    if skjerming.er_skjermet() {
        let kode_satt = journalpost
            .tilgangskode
            .as_ref()
            .is_some_and(|k| !k.trim().is_empty());
        let hjemmel_satt = journalpost
            .tilgangshjemmel
            .as_ref()
            .is_some_and(|h| !h.trim().is_empty());
        // Internt notat har ingen avsender/mottaker; da er party-kravet
        // trivielt oppfylt. Når parter finnes, må alle være unntatt offentlighet.
        let alle_unntatt = journalpost
            .avsendere_mottakere
            .as_ref()
            .is_none_or(|parter| {
                parter
                    .iter()
                    .all(|part| part.unntatt_offentlighet == Some(true))
            });

        if !(kode_satt && hjemmel_satt && alle_unntatt) {
            // Postcondition i vår egen mapping, ikke noe klienten kan rette.
            return Err(EksekveringFeil::intern(
                "arkivmapping_skjerming_postcondition_brutt",
            ));
        }
    }

    tracing::info!(
        client_reference = %client_reference,
        shielded = skjerming.er_skjermet(),
        "arkivmapping_skjerming_verifisert"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alle_sikri_koder_har_en_klientvendt_feilkode() {
        // En ny kode uten oppføring faller til ProcessingFailed i drift.
        // Denne testen tvinger noen til å ta stilling til hva klienten skal
        // se før koden rekker å nå dit.
        for kode in sikri_client::ALLE_SIKRI_KODER {
            assert!(
                error_code_for(kode).is_some(),
                "{kode} mangler oversettelse til en klientvendt feilkode"
            );
        }
    }

    #[test]
    fn arkivmapping_feil_peker_paa_client_reference() {
        // Uten referansen vet ikke klienten hvilket dokument eller hvilken
        // korrespondansepart som er feil.
        let client_reference = uuid::Uuid::from_u128(7);
        let feil = arkivmapping_feil(
            "arkivmapping_mottaker_mangler",
            "Mottaker mangler.",
            client_reference,
        );

        assert!(!feil.er_recoverable());
        assert_eq!(feil.kode, "arkivmapping_mottaker_mangler");
        assert_eq!(feil.error_code, StatusErrorCode::InvalidRequest);
        assert!(feil.melding.contains(&client_reference.to_string()));
    }

    #[test]
    fn sikri_feil_beholder_klassifisering_kode_og_melding() {
        let feil = fra_sikri(SikriFeil::irrecoverable(
            "sikri_resource_not_found",
            "Fant ikke ressursen.",
        ));

        assert!(!feil.er_recoverable());
        assert_eq!(feil.kode, "sikri_resource_not_found");
        assert_eq!(feil.error_code, StatusErrorCode::NotFound);
        assert_eq!(feil.melding, "Fant ikke ressursen.");
    }
}
