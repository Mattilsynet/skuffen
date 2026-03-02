use application::command::ports::eksekvering_port::{
    ArkivGateway, OpprettJournalpostResultat, Utsendingsvalg,
};
use async_trait::async_trait;
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
pub struct SikriArkivGateway;

impl SikriArkivGateway {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SikriArkivGateway {
    fn default() -> Self {
        Self::new()
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
            Command::OpprettInngåendeJournalpost(data) => self.opprett_inngaende(data),
            Command::OpprettUtgåendeJournalpost(data) => self.opprett_utgaaende(data, utsending),
            Command::OpprettInterntNotatJournalpost(data) => self.opprett_internt_notat(data),
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
        _command: &CommandEnvelope<Command>,
        journalpost_id: i32,
        dokument_ids: Vec<uuid::Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        let vedlegg: Vec<ElementsDokument> = dokument_ids
            .into_iter()
            .map(|_| ElementsDokument {
                tittel: None,
                filtype: None,
                innhold: None,
            })
            .collect();

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
    fn opprett_inngaende(&self, data: &OpprettInngåendeJournalpost) -> ElementsJournalpost {
        let dokumenter = self.map_dokumenter(&data.felles.dokumenter);
        ElementsJournalpost {
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
        }
    }

    fn opprett_utgaaende(
        &self,
        data: &OpprettUgåendeJournalpost,
        utsending: Option<Utsendingsvalg>,
    ) -> ElementsJournalpost {
        let dokumenter = self.map_dokumenter(&data.felles.dokumenter);
        let forsendelsesmetode = match utsending {
            Some(Utsendingsvalg::MedUtsending) => Some("GENERELL".to_string()),
            Some(Utsendingsvalg::UtenUtsending) => Some("DIG".to_string()),
            None => None,
        };

        ElementsJournalpost {
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
        }
    }

    fn opprett_internt_notat(&self, data: &OpprettInterntNotatJournalpost) -> ElementsJournalpost {
        let dokumenter = self.map_dokumenter(&data.felles.dokumenter);
        ElementsJournalpost {
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
        }
    }

    fn map_dokumenter(&self, dokumenter: &[Dokument]) -> Vec<ElementsDokument> {
        dokumenter
            .iter()
            .map(|d| ElementsDokument {
                tittel: Some(d.tittel.clone()),
                filtype: Some(d.filtype.clone()),
                innhold: None,
            })
            .collect()
    }
}
