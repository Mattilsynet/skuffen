use crate::command::media::MediaStore;
use application::command::ports::eksekvering_port::{
    ArkivGateway, OpprettJournalpostResultat, Utsendingsvalg,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    OpprettInngåendeJournalpost, OpprettInterntNotatJournalpost, OpprettUgåendeJournalpost,
};
use lib_schemas::skuffen::dokument::Dokument;
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
            sakstittel: data.sakstittel.to_string(),
            arkivdel: match data.arkivdel {
                lib_schemas::skuffen::command::sak::Arkivdel::Tilsynsdivisjonene => {
                    sikri_client::domain::ny_sak::Arkivdel::Tilsynsdivisjonene
                }
                lib_schemas::skuffen::command::sak::Arkivdel::Hovedkontoret => {
                    sikri_client::domain::ny_sak::Arkivdel::Hovedkontoret
                }
            },
            saksbehandler_id: data.saksbehandler_id.clone(),
            saksbehandler_enhet: data.saksbehandler_enhet.clone(),
            ordningsverdi: format!("{:?}", data.ordningsverdi)
                .trim_start_matches("Ordningsverdi(\"")
                .trim_end_matches("\")")
                .to_string(),
            tilgang: data
                .tilgang
                .as_ref()
                .map(|t| sikri_client::domain::ny_sak::Tilgang {
                    tilgangskode: t.tilgangskode.clone(),
                    tilgangshjemmel: t.tilgangshjemmel.clone(),
                }),
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
        saksnummer: &str,
        utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        let journalpost = match &command.payload {
            Command::OpprettInngåendeJournalpost(data) => self.opprett_inngaende(data).await?,
            Command::OpprettUtgåendeJournalpost(data) => {
                self.opprett_utgaaende(data, utsending).await?
            }
            Command::OpprettInterntNotatJournalpost(data) => {
                self.opprett_internt_notat(data).await?
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
}

impl SikriArkivGateway {
    async fn opprett_inngaende(
        &self,
        data: &OpprettInngåendeJournalpost,
    ) -> Result<ElementsJournalpost, anyhow::Error> {
        let dokumenter = self.map_dokumenter(&data.felles.dokumenter).await?;
        Ok(ElementsJournalpost {
            tittel: Some(data.felles.tittel.clone()),
            journalposttype: Some("I".to_string()),
            journalstatus: Some("J".to_string()),
            avskriv_direkte: Some(true),
            avskrivningsmaate: Some("TE".to_string()),
            tilgangskode: data.felles.tilgang.as_ref().map(|t| t.tilgangskode.clone()),
            tilgangshjemmel: data
                .felles
                .tilgang
                .as_ref()
                .map(|t| t.tilgangshjemmel.clone()),
            saksbehandler: Some(data.felles.saksbehandler.clone()),
            saksbehandler_enhet: Some(data.felles.saksbehandler_enhet.clone()),
            avsendere_mottakere: Some(vec![ElementsAvsenderMottaker {
                er_mottaker: Some(false),
                navn: Some(data.avsender.clone()),
                forsendelsesmetode: None,
                kopi: None,
                unntatt_offentlighet: None,
                person: None,
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
            }]),
            dokumenter: Some(dokumenter),
            dokument_dato: Some(data.felles.dokument_dato.clone()),
        })
    }

    async fn opprett_utgaaende(
        &self,
        data: &OpprettUgåendeJournalpost,
        utsending: Option<Utsendingsvalg>,
    ) -> Result<ElementsJournalpost, anyhow::Error> {
        let dokumenter = self.map_dokumenter(&data.felles.dokumenter).await?;
        let forsendelsesmetode = match utsending {
            Some(Utsendingsvalg::MedUtsending) => Some("GENERELL".to_string()),
            Some(Utsendingsvalg::UtenUtsending) => Some("DIG".to_string()),
            None => None,
        };

        Ok(ElementsJournalpost {
            tittel: Some(data.felles.tittel.clone()),
            journalposttype: Some("U".to_string()),
            journalstatus: Some("R".to_string()),
            avskriv_direkte: None,
            avskrivningsmaate: None,
            tilgangskode: data.felles.tilgang.as_ref().map(|t| t.tilgangskode.clone()),
            tilgangshjemmel: data
                .felles
                .tilgang
                .as_ref()
                .map(|t| t.tilgangshjemmel.clone()),
            saksbehandler: Some(data.felles.saksbehandler.clone()),
            saksbehandler_enhet: Some(data.felles.saksbehandler_enhet.clone()),
            avsendere_mottakere: Some(vec![ElementsAvsenderMottaker {
                er_mottaker: Some(true),
                navn: Some(data.mottaker.clone()),
                forsendelsesmetode,
                kopi: None,
                unntatt_offentlighet: None,
                person: None,
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
            }]),
            dokumenter: Some(dokumenter),
            dokument_dato: Some(data.felles.dokument_dato.clone()),
        })
    }

    async fn opprett_internt_notat(
        &self,
        data: &OpprettInterntNotatJournalpost,
    ) -> Result<ElementsJournalpost, anyhow::Error> {
        let dokumenter = self.map_dokumenter(&data.felles.dokumenter).await?;
        Ok(ElementsJournalpost {
            tittel: Some(data.felles.tittel.clone()),
            journalposttype: Some("X".to_string()),
            journalstatus: Some("J".to_string()),
            avskriv_direkte: None,
            avskrivningsmaate: None,
            tilgangskode: data.felles.tilgang.as_ref().map(|t| t.tilgangskode.clone()),
            tilgangshjemmel: data
                .felles
                .tilgang
                .as_ref()
                .map(|t| t.tilgangshjemmel.clone()),
            saksbehandler: Some(data.felles.saksbehandler.clone()),
            saksbehandler_enhet: Some(data.felles.saksbehandler_enhet.clone()),
            avsendere_mottakere: None,
            dokumenter: Some(dokumenter),
            dokument_dato: Some(data.felles.dokument_dato.clone()),
        })
    }

    async fn map_dokumenter(
        &self,
        dokumenter: &[Dokument],
    ) -> Result<Vec<ElementsDokument>, anyhow::Error> {
        let mut mapped = Vec::with_capacity(dokumenter.len());
        for (index, d) in dokumenter.iter().enumerate() {
            let innhold = self.hent_media_base64(d.dokument_referanse).await?;
            mapped.push(ElementsDokument {
                tittel: Some(d.tittel.clone()),
                hoveddokument: index == 0,
                filtype: Some(d.filtype.clone()),
                innhold: Some(innhold),
            });
        }
        Ok(mapped)
    }

    async fn map_vedlegg_dokument(
        &self,
        command: &CommandEnvelope<Command>,
        dokument_id: uuid::Uuid,
    ) -> Result<ElementsDokument, anyhow::Error> {
        let dokument = Self::dokument_for_client_reference(command, dokument_id)?;
        let innhold = self.hent_media_base64(dokument.dokument_referanse).await?;
        Ok(ElementsDokument {
            tittel: Some(dokument.tittel.clone()),
            hoveddokument: false,
            filtype: Some(dokument.filtype.clone()),
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

    fn dokument_for_client_reference<'a>(
        command: &'a CommandEnvelope<Command>,
        dokument_id: uuid::Uuid,
    ) -> Result<&'a Dokument, anyhow::Error> {
        let dokumenter = match &command.payload {
            Command::OpprettInngåendeJournalpost(data) => &data.felles.dokumenter,
            Command::OpprettUtgåendeJournalpost(data) => &data.felles.dokumenter,
            Command::OpprettInterntNotatJournalpost(data) => &data.felles.dokumenter,
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
