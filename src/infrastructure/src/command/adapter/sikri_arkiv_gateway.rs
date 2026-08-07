use crate::command::media::MediaStore;
use application::command::ports::eksekvering_port::{
    ArkivGateway, OpprettJournalpostResultat, Utsendingsvalg,
};
use application::command::{
    Arkivdel, Command, CommandEnvelope, Dokument, Dokumentform, Korrespondansepart, MottakerId,
    OpprettJournalpostCommand, Parttype, Tilgjengelighet, Utsendingsmottaker,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, JournalpostMedDokumenter,
};
use sikri_client::domain::ny_sak::NySak;
use sikri_client::dto::elements_avsender_mottaker::ElementsAvsenderMottaker;
use sikri_client::dto::elements_dokument::ElementsDokument;
use sikri_client::dto::elements_journalpost::ElementsJournalpost;

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
        command: &CommandEnvelope<Command>,
    ) -> Result<String, anyhow::Error> {
        let Command::OpprettSak(data) = &command.payload else {
            return Err(anyhow::anyhow!("Ugyldig kommando for opprett_sak"));
        };

        let ny_sak = NySak {
            sakstittel: data.sakstittel.clone(),
            arkivdel: match data.arkivdel {
                Arkivdel::Tilsynsdivisjonene => {
                    sikri_client::domain::ny_sak::Arkivdel::Tilsynsdivisjonene
                }
                Arkivdel::Hovedkontoret => sikri_client::domain::ny_sak::Arkivdel::Hovedkontoret,
            },
            saksbehandler_id: data.saksbehandler_id.clone(),
            saksbehandler_enhet: data.saksbehandler_enhet.clone(),
            ordningsverdi: data.ordningsverdi.get().to_string(),
            tilgang: match &data.tilgjengelighet {
                Tilgjengelighet::Skjermet {
                    tilgangskode,
                    tilgangshjemmel,
                } => Some(sikri_client::domain::ny_sak::Tilgang {
                    tilgangskode: tilgangskode.as_str().to_string(),
                    tilgangshjemmel: tilgangshjemmel.as_str().to_string(),
                }),
                Tilgjengelighet::Offentlig => None,
            },
            virksomhetsmappe_id: None,
        };

        let resp = sikri_client::opprett_sak(ny_sak).await?;
        let saksnummer = resp
            .saksnr
            .ok_or_else(|| anyhow::anyhow!("Saksnummer mangler i respons"))?;
        Ok(saksnummer)
    }

    async fn opprett_journalpost(
        &self,
        command: &CommandEnvelope<Command>,
        journalpost: &JournalpostMedDokumenter,
        saksnummer: &str,
        utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        let journalpost = match &command.payload {
            Command::OpprettInngaaendeJournalpost(data) => {
                self.opprett_inngaende(data, journalpost).await?
            }
            Command::OpprettUtgaaendeJournalpost(data) => {
                self.opprett_utgaaende(data, journalpost, utsending).await?
            }
            Command::OpprettInterntNotatJournalpost(data) => {
                self.opprett_internt_notat(data, journalpost).await?
            }
            _ => return Err(anyhow::anyhow!("Ugyldig kommando for opprett_journalpost")),
        };

        let resp = sikri_client::opprett_journalpost(journalpost, saksnummer).await?;
        let journalpost_id = resp
            .journalpost_id
            .ok_or_else(|| anyhow::anyhow!("JournalpostId mangler i respons"))?;
        Ok(OpprettJournalpostResultat { journalpost_id })
    }

    async fn legg_til_vedlegg(
        &self,
        command: &CommandEnvelope<Command>,
        journalpost_id: i32,
        dokument_ids: Vec<uuid::Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        let mut vedlegg: Vec<ElementsDokument> = Vec::with_capacity(dokument_ids.len());
        for dokument_id in dokument_ids {
            vedlegg.push(self.map_vedlegg_dokument(command, dokument_id).await?);
        }

        let resp = sikri_client::legg_til_vedlegg(journalpost_id, vedlegg).await?;
        Ok(resp.into_iter().map(|d| d.dokument_id).collect())
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: &str,
    ) -> Result<(), anyhow::Error> {
        sikri_client::sett_journalpost_status(journalpost_id, status).await
    }

    async fn avskriv_journalpost(
        &self,
        journalpost_id: i32,
        avskrivingsmaate: &str,
    ) -> Result<(), anyhow::Error> {
        sikri_client::avskriv_journalpost(journalpost_id, avskrivingsmaate).await
    }

    async fn avslutt_sak(&self, saksnummer: &str) -> Result<(), anyhow::Error> {
        sikri_client::avslutt_sak(saksnummer).await
    }

    async fn sett_saksansvarlig(
        &self,
        saksnummer: &str,
        saksbehandler: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        sikri_client::sett_saksansvarlig(saksnummer, saksbehandler, saksbehandler_enhet).await
    }
}

impl SikriArkivGateway {
    async fn opprett_inngaende(
        &self,
        data: &OpprettJournalpostCommand,
        journalpost: &JournalpostMedDokumenter,
    ) -> Result<ElementsJournalpost, anyhow::Error> {
        let dokumenter = self
            .map_dokumenter(&data.felles().dokumenter, journalpost)
            .await?;
        let OpprettJournalpostCommand::Inngaende { avsender, .. } = data else {
            return Err(anyhow::anyhow!(
                "arkivmapping_feil_variant client_reference={} sikri_recoverability=irrecoverable",
                data.felles().client_reference
            ));
        };
        let skjerming = skjerming_fra_tilgjengelighet(
            &data.felles().tilgjengelighet,
            data.felles().client_reference,
        )?;

        let elements = ElementsJournalpost {
            tittel: Some(data.felles().tittel.clone()),
            journalposttype: Some("I".to_string()),
            journalstatus: Some("J".to_string()),
            avskriv_direkte: Some(true),
            avskrivningsmaate: Some("TE".to_string()),
            tilgangskode: skjerming.tilgangskode(),
            tilgangshjemmel: skjerming.tilgangshjemmel(),
            saksbehandler: Some(data.felles().saksbehandler.clone()),
            saksbehandler_enhet: Some(data.felles().saksbehandler_enhet.clone()),
            avsendere_mottakere: Some(vec![korrespondansepart_avsender_mottaker(
                avsender, false, &skjerming,
            )?]),
            dokumenter: Some(dokumenter),
            dokument_dato: Some(data.felles().dokument_dato.clone()),
        };

        verifiser_skjerming(&elements, &skjerming, data.felles().client_reference)?;
        Ok(elements)
    }

    async fn opprett_utgaaende(
        &self,
        data: &OpprettJournalpostCommand,
        journalpost: &JournalpostMedDokumenter,
        utsending: Option<Utsendingsvalg>,
    ) -> Result<ElementsJournalpost, anyhow::Error> {
        let dokumenter = self
            .map_dokumenter(&data.felles().dokumenter, journalpost)
            .await?;
        let forsendelsesmetode = match utsending {
            Some(Utsendingsvalg::MedUtsending) => Some("GENERELL".to_string()),
            Some(Utsendingsvalg::UtenUtsending) => Some("DIG".to_string()),
            None => None,
        };
        let skjerming = skjerming_fra_tilgjengelighet(
            &data.felles().tilgjengelighet,
            data.felles().client_reference,
        )?;

        let mut avsendere_mottakere: Vec<ElementsAvsenderMottaker> = Vec::new();
        match data {
            OpprettJournalpostCommand::Utgaaende { mottakere, .. } => {
                for mottaker in mottakere {
                    let mut am = korrespondansepart_avsender_mottaker(mottaker, true, &skjerming)?;
                    am.forsendelsesmetode = forsendelsesmetode.clone();
                    avsendere_mottakere.push(am);
                }
            }
            OpprettJournalpostCommand::UtgaaendeMedUtsending { mottakere, .. } => {
                for mottaker in mottakere {
                    avsendere_mottakere.push(utsendingsmottaker_avsender_mottaker(
                        mottaker,
                        &skjerming,
                        data.felles().client_reference,
                    )?);
                }
            }
            _ => {
                return Err(anyhow::anyhow!(
                    "arkivmapping_feil_variant client_reference={} sikri_recoverability=irrecoverable",
                    data.felles().client_reference
                ));
            }
        }

        if avsendere_mottakere.is_empty() {
            return Err(anyhow::anyhow!(
                "arkivmapping_mottaker_mangler client_reference={} sikri_recoverability=irrecoverable",
                data.felles().client_reference
            ));
        }

        let elements = ElementsJournalpost {
            tittel: Some(data.felles().tittel.clone()),
            journalposttype: Some("U".to_string()),
            journalstatus: Some("R".to_string()),
            avskriv_direkte: None,
            avskrivningsmaate: None,
            tilgangskode: skjerming.tilgangskode(),
            tilgangshjemmel: skjerming.tilgangshjemmel(),
            saksbehandler: Some(data.felles().saksbehandler.clone()),
            saksbehandler_enhet: Some(data.felles().saksbehandler_enhet.clone()),
            avsendere_mottakere: Some(avsendere_mottakere),
            dokumenter: Some(dokumenter),
            dokument_dato: Some(data.felles().dokument_dato.clone()),
        };

        verifiser_skjerming(&elements, &skjerming, data.felles().client_reference)?;
        Ok(elements)
    }

    async fn opprett_internt_notat(
        &self,
        data: &OpprettJournalpostCommand,
        journalpost: &JournalpostMedDokumenter,
    ) -> Result<ElementsJournalpost, anyhow::Error> {
        let dokumenter = self
            .map_dokumenter(&data.felles().dokumenter, journalpost)
            .await?;
        let skjerming = skjerming_fra_tilgjengelighet(
            &data.felles().tilgjengelighet,
            data.felles().client_reference,
        )?;

        let elements = ElementsJournalpost {
            tittel: Some(data.felles().tittel.clone()),
            journalposttype: Some("X".to_string()),
            journalstatus: Some("J".to_string()),
            avskriv_direkte: None,
            avskrivningsmaate: None,
            tilgangskode: skjerming.tilgangskode(),
            tilgangshjemmel: skjerming.tilgangshjemmel(),
            saksbehandler: Some(data.felles().saksbehandler.clone()),
            saksbehandler_enhet: Some(data.felles().saksbehandler_enhet.clone()),
            avsendere_mottakere: None,
            dokumenter: Some(dokumenter),
            dokument_dato: Some(data.felles().dokument_dato.clone()),
        };

        verifiser_skjerming(&elements, &skjerming, data.felles().client_reference)?;
        Ok(elements)
    }

    async fn map_dokumenter(
        &self,
        dokumenter: &[Dokument],
        journalpost: &JournalpostMedDokumenter,
    ) -> Result<Vec<ElementsDokument>, anyhow::Error> {
        let Some(hoveddokument) = dokumenter.first() else {
            return Ok(Vec::new());
        };

        let (dokument_referanse, filtype) = hoveddokument_referanse(hoveddokument, journalpost)?;
        let innhold = self.hent_media_base64(dokument_referanse).await?;
        Ok(vec![ElementsDokument {
            tittel: Some(hoveddokument.tittel.clone()),
            hoveddokument: true,
            filtype: Some(filtype.to_string()),
            innhold: Some(innhold),
        }])
    }

    async fn map_vedlegg_dokument(
        &self,
        command: &CommandEnvelope<Command>,
        dokument_id: uuid::Uuid,
    ) -> Result<ElementsDokument, anyhow::Error> {
        let dokument = Self::dokument_for_client_reference(command, dokument_id)?;
        let (dokument_referanse, filtype) = bytes_form(dokument)?;
        let innhold = self.hent_media_base64(dokument_referanse).await?;
        Ok(ElementsDokument {
            tittel: Some(dokument.tittel.clone()),
            hoveddokument: false,
            filtype: Some(filtype.to_string()),
            innhold: Some(innhold),
        })
    }

    async fn hent_media_base64(
        &self,
        dokument_referanse: uuid::Uuid,
    ) -> Result<String, anyhow::Error> {
        let media = self
            .media_store
            .get(dokument_referanse)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("Media mangler for dokument_referanse={dokument_referanse}")
            })?;
        Ok(STANDARD.encode(media.data))
    }

    fn dokument_for_client_reference(
        command: &CommandEnvelope<Command>,
        dokument_id: uuid::Uuid,
    ) -> Result<&Dokument, anyhow::Error> {
        let dokumenter = match &command.payload {
            Command::OpprettInngaaendeJournalpost(data) => &data.felles().dokumenter,
            Command::OpprettUtgaaendeJournalpost(data) => &data.felles().dokumenter,
            Command::OpprettInterntNotatJournalpost(data) => &data.felles().dokumenter,
            _ => return Err(anyhow::anyhow!("Ugyldig kommando for dokumentmapping")),
        };

        dokumenter
            .iter()
            .find(|d| d.client_reference == dokument_id)
            .ok_or_else(|| {
                anyhow::anyhow!("Dokument med client_reference={dokument_id} ble ikke funnet")
            })
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

fn skjerming_fra_tilgjengelighet(
    tilgjengelighet: &Tilgjengelighet,
    _client_reference: uuid::Uuid,
) -> Result<Skjerming, anyhow::Error> {
    match tilgjengelighet {
        Tilgjengelighet::Offentlig => Ok(Skjerming::Offentlig),
        Tilgjengelighet::Skjermet {
            tilgangskode,
            tilgangshjemmel,
        } => {
            // tilgangskode/tilgangshjemmel er validerte newtypes (non-empty),
            // så vi kan stole på verdiene her.
            Ok(Skjerming::Skjermet {
                tilgangskode: tilgangskode.as_str().to_string(),
                tilgangshjemmel: tilgangshjemmel.as_str().to_string(),
            })
        }
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
) -> Result<ElementsAvsenderMottaker, anyhow::Error> {
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
) -> Result<ElementsAvsenderMottaker, anyhow::Error> {
    // postnummer er en validert Postnummer-newtype (4 siffer), så kun de rå
    // String-feltene må sjekkes for tomhet her.
    if mottaker.adresse.adresse.trim().is_empty() || mottaker.adresse.poststed.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "arkivmapping_postadresse_mangler client_reference={client_reference} sikri_recoverability=irrecoverable"
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
) -> Result<(), anyhow::Error> {
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
            return Err(anyhow::anyhow!(
                "arkivmapping_skjerming_postcondition_brutt client_reference={client_reference} sikri_recoverability=irrecoverable"
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
fn hoveddokument_referanse(
    dokument: &Dokument,
    journalpost: &JournalpostMedDokumenter,
) -> Result<(uuid::Uuid, String), anyhow::Error> {
    match &dokument.form {
        Dokumentform::Bytes {
            dokument_referanse,
            filtype,
        } => Ok((*dokument_referanse, filtype.clone())),
        Dokumentform::HtmlTemplate { .. } => {
            // v1 maps only the command's first document as Sikri hoveddokument.
            // The persisted document fact uses Skuffen's internal ID, not the
            // command client_reference, so the hoveddokument fact is selected by
            // the same positional invariant.
            let tilstand = journalpost.dokumenter.first().ok_or_else(|| {
                anyhow::anyhow!(
                    "arkivmapping_dokument_fact_mangler dokument_client_reference={} sikri_recoverability=irrecoverable",
                    dokument.client_reference
                )
            })?;
            rendered_template_referanse(tilstand)
        }
    }
}

fn rendered_template_referanse(
    dokument: &DokumentMedTilstand,
) -> Result<(uuid::Uuid, String), anyhow::Error> {
    match &dokument.kilde {
        DokumentKildeTilstand::HtmlTemplate {
            rendered_dokument_referanse: Some(rendered),
            ..
        } => Ok((*rendered, "PDF".to_string())),
        DokumentKildeTilstand::HtmlTemplate {
            rendered_dokument_referanse: None,
            ..
        } => Err(anyhow::anyhow!(
            "arkivmapping_rendered_dokument_mangler dokument_id={} sikri_recoverability=irrecoverable",
            dokument.dokument_id.0
        )),
        DokumentKildeTilstand::Bytes => Err(anyhow::anyhow!(
            "arkivmapping_dokumentform_mismatch dokument_id={} sikri_recoverability=irrecoverable",
            dokument.dokument_id.0
        )),
    }
}

fn bytes_form(dokument: &Dokument) -> Result<(uuid::Uuid, &str), anyhow::Error> {
    match &dokument.form {
        Dokumentform::Bytes {
            dokument_referanse,
            filtype,
        } => Ok((*dokument_referanse, filtype.as_str())),
        Dokumentform::HtmlTemplate { .. } => Err(anyhow::anyhow!(
            "arkivmapping_dokumentform_mismatch dokument_client_reference={} sikri_recoverability=irrecoverable",
            dokument.client_reference
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{SikriArkivGateway, Skjerming, hoveddokument_referanse, verifiser_skjerming};
    use crate::command::media::{MediaFile, MediaMetadata, MediaStore};
    use application::command::{
        Command, CommandEnvelope, Dokument, Dokumentform, JournalpostCommon,
        OpprettJournalpostCommand, SakKey, Tilgjengelighet,
    };
    use async_trait::async_trait;
    use domain::eksekvering::html_template::TemplateFelt;
    use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId};
    use domain::eksekvering::tilstand::{
        DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
        JournalpostTilstand, JournalpostType,
    };
    use sikri_client::dto::elements_avsender_mottaker::ElementsAvsenderMottaker;
    use sikri_client::dto::elements_journalpost::ElementsJournalpost;
    use std::collections::HashMap;
    use std::sync::Arc;
    use uuid::Uuid;

    struct FakeMediaStore {
        files: HashMap<Uuid, MediaFile>,
    }

    impl FakeMediaStore {
        fn with_files(files: Vec<MediaFile>) -> Self {
            Self {
                files: files.into_iter().map(|file| (file.id, file)).collect(),
            }
        }
    }

    #[async_trait]
    impl MediaStore for FakeMediaStore {
        async fn save(&self, _file: MediaFile) -> Result<(), anyhow::Error> {
            Ok(())
        }

        async fn exists(&self, id: Uuid) -> Result<bool, anyhow::Error> {
            Ok(self.files.contains_key(&id))
        }

        async fn get(&self, id: Uuid) -> Result<Option<MediaFile>, anyhow::Error> {
            Ok(self.files.get(&id).cloned())
        }
    }

    #[tokio::test]
    async fn create_mapping_only_includes_first_document_as_hoveddokument() {
        let hoveddokument = sample_document("Rapport", "PDF");
        let vedlegg = sample_document("Vedlegg", "PNG");
        let gateway = sample_gateway(&[&hoveddokument, &vedlegg]);
        let journalpost = sample_journalpost_for_documents(&[&hoveddokument, &vedlegg]);

        let mapped = gateway
            .map_dokumenter(&[hoveddokument.clone(), vedlegg], &journalpost)
            .await
            .expect("documents should map");

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].tittel.as_deref(), Some("Rapport"));
        assert_eq!(mapped[0].filtype.as_deref(), Some("PDF"));
        assert!(mapped[0].hoveddokument);
    }

    #[tokio::test]
    async fn html_template_hoveddokument_bruker_rendered_pdf() {
        let rendered_id = Uuid::new_v4();
        let dokument = sample_html_template_document();
        let gateway = sample_gateway_with_files(vec![MediaFile {
            id: rendered_id,
            data: b"rendered pdf".to_vec(),
            filename: Some("rendered.pdf".to_string()),
            content_type: Some("application/pdf".to_string()),
            metadata: MediaMetadata::default(),
        }]);
        let journalpost = sample_journalpost(vec![sample_html_template_fact(
            dokument.client_reference,
            Some(rendered_id),
        )]);

        let mapped = gateway
            .map_dokumenter(std::slice::from_ref(&dokument), &journalpost)
            .await
            .expect("rendered template should map");

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].tittel.as_deref(), Some("HTML-template"));
        assert_eq!(mapped[0].filtype.as_deref(), Some("PDF"));
        assert_eq!(mapped[0].innhold.as_deref(), Some("cmVuZGVyZWQgcGRm"));
        assert!(mapped[0].hoveddokument);
    }

    #[tokio::test]
    async fn html_template_hoveddokument_bruker_forste_dokumentfact_selv_om_id_er_intern() {
        let rendered_id = Uuid::new_v4();
        let dokument = sample_html_template_document();
        let gateway = sample_gateway_with_files(vec![MediaFile {
            id: rendered_id,
            data: b"rendered pdf".to_vec(),
            filename: Some("rendered.pdf".to_string()),
            content_type: Some("application/pdf".to_string()),
            metadata: MediaMetadata::default(),
        }]);
        let journalpost = sample_journalpost(vec![sample_html_template_fact(
            Uuid::new_v4(),
            Some(rendered_id),
        )]);

        let mapped = gateway
            .map_dokumenter(std::slice::from_ref(&dokument), &journalpost)
            .await
            .expect("rendered template should map by hoveddokument position");

        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].filtype.as_deref(), Some("PDF"));
        assert_eq!(mapped[0].innhold.as_deref(), Some("cmVuZGVyZWQgcGRm"));
        assert!(mapped[0].hoveddokument);
    }

    #[test]
    fn html_template_hoveddokument_krever_rendered_reference() {
        let dokument = sample_html_template_document();
        let journalpost = sample_journalpost(vec![sample_html_template_fact(
            dokument.client_reference,
            None,
        )]);

        let err =
            hoveddokument_referanse(&dokument, &journalpost).expect_err("missing rendered ref");

        assert!(
            err.to_string()
                .starts_with("arkivmapping_rendered_dokument_mangler")
        );
    }

    #[tokio::test]
    async fn attachment_mapping_marks_document_as_not_hoveddokument() {
        let hoveddokument = sample_document("Rapport", "PDF");
        let vedlegg = sample_document("Vedlegg", "PNG");
        let gateway = sample_gateway(&[&hoveddokument, &vedlegg]);
        let command = sample_command(vec![hoveddokument, vedlegg.clone()]);

        let mapped = gateway
            .map_vedlegg_dokument(&command, vedlegg.client_reference)
            .await
            .expect("attachment should map");

        assert_eq!(mapped.tittel.as_deref(), Some("Vedlegg"));
        assert_eq!(mapped.filtype.as_deref(), Some("PNG"));
        assert!(!mapped.hoveddokument);
    }

    #[tokio::test]
    async fn html_template_attachment_mapping_returns_irrecoverable_mapping_error() {
        let dokument = sample_html_template_document();
        let command = sample_command(vec![dokument.clone()]);
        let gateway = sample_gateway_with_files(Vec::new());

        let err = gateway
            .map_vedlegg_dokument(&command, dokument.client_reference)
            .await
            .expect_err("html template attachment should fail before media lookup");

        let message = err.to_string();
        assert!(message.starts_with("arkivmapping_dokumentform_mismatch"));
        assert!(message.contains(&format!(
            "dokument_client_reference={}",
            dokument.client_reference
        )));
        assert!(message.contains("sikri_recoverability=irrecoverable"));
    }

    fn sample_gateway(dokumenter: &[&Dokument]) -> SikriArkivGateway {
        let files = dokumenter
            .iter()
            .map(|dokument| MediaFile {
                id: dokument_referanse(dokument),
                data: dokument.tittel.as_bytes().to_vec(),
                filename: Some(format!("{}.{}", dokument.tittel, filtype(dokument))),
                content_type: None,
                metadata: MediaMetadata::default(),
            })
            .collect();
        SikriArkivGateway::new(Arc::new(FakeMediaStore::with_files(files)))
    }

    fn sample_gateway_with_files(files: Vec<MediaFile>) -> SikriArkivGateway {
        SikriArkivGateway::new(Arc::new(FakeMediaStore::with_files(files)))
    }

    fn sample_journalpost_for_documents(dokumenter: &[&Dokument]) -> JournalpostMedDokumenter {
        sample_journalpost(
            dokumenter
                .iter()
                .map(|dokument| match &dokument.form {
                    Dokumentform::Bytes { .. } => DokumentMedTilstand {
                        dokument_id: SkuffenDokumentId::from(dokument.client_reference),
                        tilstand: DokumentTilstand::IkkeRealisert,
                        kilde: DokumentKildeTilstand::Bytes,
                    },
                    Dokumentform::HtmlTemplate {
                        mal_referanse,
                        felter,
                    } => DokumentMedTilstand {
                        dokument_id: SkuffenDokumentId::from(dokument.client_reference),
                        tilstand: DokumentTilstand::Ok,
                        kilde: DokumentKildeTilstand::HtmlTemplate {
                            mal_referanse: *mal_referanse,
                            felter: felter.clone(),
                            rendered_dokument_referanse: Some(Uuid::new_v4()),
                        },
                    },
                })
                .collect(),
        )
    }

    fn sample_journalpost(dokumenter: Vec<DokumentMedTilstand>) -> JournalpostMedDokumenter {
        JournalpostMedDokumenter {
            journalpost_id: SkuffenJournalpostId::from(Uuid::new_v4()),
            journalposttype: JournalpostType::InterntNotat,
            med_utsending: false,
            tilstand: JournalpostTilstand::IkkeRealisert,
            sikri_id: None,
            journalpostnummer: None,
            dokumenter,
        }
    }

    fn sample_html_template_fact(
        dokument_id: Uuid,
        rendered_dokument_referanse: Option<Uuid>,
    ) -> DokumentMedTilstand {
        DokumentMedTilstand {
            dokument_id: SkuffenDokumentId::from(dokument_id),
            tilstand: if rendered_dokument_referanse.is_some() {
                DokumentTilstand::Ok
            } else {
                DokumentTilstand::AvventerRendring
            },
            kilde: DokumentKildeTilstand::HtmlTemplate {
                mal_referanse: Uuid::new_v4(),
                felter: vec![TemplateFelt::Saksnummer],
                rendered_dokument_referanse,
            },
        }
    }

    fn sample_command(dokumenter: Vec<Dokument>) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettInterntNotatJournalpost(
                OpprettJournalpostCommand::InterntNotat {
                    felles: JournalpostCommon {
                        client_reference: Uuid::new_v4(),
                        tittel: "Internt notat".to_string(),
                        dokument_dato: "2025-01-01".to_string(),
                        saksbehandler: "Z12345".to_string(),
                        saksbehandler_enhet: "1234".to_string(),
                        tilgjengelighet: Tilgjengelighet::Offentlig,
                        dokumenter,
                        sak_key: SakKey::ClientReference(Uuid::new_v4()),
                        kildesystem: None,
                    },
                },
            ),
        }
    }

    fn sample_document(tittel: &str, filtype: &str) -> Dokument {
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: tittel.to_string(),
            form: Dokumentform::Bytes {
                filtype: filtype.to_string(),
                dokument_referanse: Uuid::new_v4(),
            },
        }
    }

    fn sample_html_template_document() -> Dokument {
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: "HTML-template".to_string(),
            form: Dokumentform::HtmlTemplate {
                mal_referanse: Uuid::new_v4(),
                felter: vec![TemplateFelt::Saksnummer],
            },
        }
    }

    fn dokument_referanse(dokument: &Dokument) -> Uuid {
        match &dokument.form {
            Dokumentform::Bytes {
                dokument_referanse,
                filtype: _,
            } => *dokument_referanse,
            Dokumentform::HtmlTemplate { .. } => panic!("expected bytes document"),
        }
    }

    fn filtype(dokument: &Dokument) -> &str {
        match &dokument.form {
            Dokumentform::Bytes {
                dokument_referanse: _,
                filtype,
            } => filtype,
            Dokumentform::HtmlTemplate { .. } => panic!("expected bytes document"),
        }
    }

    fn skjermet_journalpost(
        avsendere_mottakere: Option<Vec<ElementsAvsenderMottaker>>,
    ) -> ElementsJournalpost {
        ElementsJournalpost {
            tittel: Some("Tittel".to_string()),
            journalposttype: Some("X".to_string()),
            journalstatus: Some("J".to_string()),
            avskriv_direkte: None,
            avskrivningsmaate: None,
            tilgangskode: Some("UO".to_string()),
            tilgangshjemmel: Some("Offl. § 13".to_string()),
            saksbehandler: Some("Z00000".to_string()),
            saksbehandler_enhet: Some("42".to_string()),
            avsendere_mottakere,
            dokumenter: None,
            dokument_dato: Some("2026-01-01".to_string()),
        }
    }

    #[test]
    fn skjermet_internt_notat_uten_parter_passerer_postcondition() {
        let journalpost = skjermet_journalpost(None);
        let skjerming = Skjerming::Skjermet {
            tilgangskode: "UO".to_string(),
            tilgangshjemmel: "Offl. § 13".to_string(),
        };

        verifiser_skjerming(&journalpost, &skjerming, uuid::Uuid::nil())
            .expect("skjermet internt notat uten parter skal passere");
    }

    #[test]
    fn skjermet_med_uskjermet_part_feiler() {
        let part = ElementsAvsenderMottaker {
            forsendelsesmetode: None,
            er_mottaker: Some(true),
            kopi: None,
            unntatt_offentlighet: Some(false),
            person: Some(false),
            til_saksbehandler: None,
            til_saksbehandler_enhet: None,
            id: None,
            navn: Some("Acme AS".to_string()),
            organisasjonsnummer: Some("995298775".to_string()),
            epost: None,
            telefon: None,
            postadresse: None,
            postnummer: None,
            poststed: None,
            utlandsadresse: None,
        };
        let journalpost = skjermet_journalpost(Some(vec![part]));
        let skjerming = Skjerming::Skjermet {
            tilgangskode: "UO".to_string(),
            tilgangshjemmel: "Offl. § 13".to_string(),
        };

        let err = verifiser_skjerming(&journalpost, &skjerming, uuid::Uuid::nil())
            .expect_err("uskjermet part på skjermet journalpost skal feile");
        assert!(
            err.to_string()
                .contains("arkivmapping_skjerming_postcondition_brutt")
        );
    }
}
