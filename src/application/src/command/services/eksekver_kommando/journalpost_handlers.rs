use crate::command::{Command, CommandEnvelope};
use domain::eksekvering::id::SkuffenJournalpostId;
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentTilstand, JournalpostMedDokumenter, JournalpostTilstand,
    SakMedBarn,
};
use domain::eksekvering::typer::EksekveringFeil;

use crate::command::ports::eksekvering_port::Utsendingsvalg;

use super::{EksekverKommandoService, extract_journalpost_client_reference};

impl EksekverKommandoService {
    pub(super) async fn opprett_journalpost(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), EksekveringFeil> {
        let jp = finn_journalpost(sak, journalpost_id)?;

        let saksnummer = sak.saksnummer.as_deref().ok_or_else(|| {
            EksekveringFeil::blocked("Saksnummer mangler for opprett_journalpost")
        })?;

        let utsending = resolve_utsending(jp);

        let resultat = self
            .arkiv_gateway
            .opprett_journalpost(envelope, jp, saksnummer, utsending)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        if let Some(client_ref) = extract_journalpost_client_reference(envelope) {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(
                    client_ref,
                    resultat.journalpost_id.to_string(),
                )
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        self.entity_tilstand_repo
            .oppdater_journalpost_tilstand(
                journalpost_id,
                JournalpostTilstand::Opprettet,
                Some(resultat.journalpost_id as i64),
                Some(resultat.journalpost_id),
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "journalpost",
                journalpost_id.0,
                envelope.command_id,
                "ikke_realisert",
                "opprettet",
                "opprett_journalpost",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        // Hoveddokument is auto-included in the Sikri call when the first document
        // is archive-ready. HTML-template hoveddokumenter are rendered to PDF
        // before OpprettJournalpost and are included from rendered facts by the
        // archive gateway.
        if let Some(hoveddokument) = jp.dokumenter.first()
            && hoveddokument.kilde == DokumentKildeTilstand::Bytes
        {
            self.entity_tilstand_repo
                .oppdater_dokument_tilstand(hoveddokument.dokument_id, DokumentTilstand::Ok)
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

            self.entity_tilstand_repo
                .logg_overgang(
                    "dokument",
                    hoveddokument.dokument_id.0,
                    envelope.command_id,
                    "ikke_realisert",
                    "ok",
                    "opprett_journalpost",
                    None,
                )
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        Ok(())
    }

    pub(super) async fn journalfoer(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), EksekveringFeil> {
        let jp = finn_journalpost(sak, journalpost_id)?;

        let journalpostnummer = jp.journalpostnummer.ok_or_else(|| {
            EksekveringFeil::blocked("Journalpostnummer mangler for journalfoering")
        })?;

        // Determine Sikri status and resulting tilstand
        let (ny_status, ny_tilstand) =
            if jp.journalposttype == JournalpostType::Utgaaende && jp.med_utsending {
                ("F", JournalpostTilstand::VenterPaaUtsending)
            } else {
                ("J", JournalpostTilstand::Journalfoert)
            };

        self.arkiv_gateway
            .sett_journalpost_status(journalpostnummer, ny_status)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        let fra_tilstand = tilstand_str(jp.tilstand);

        self.entity_tilstand_repo
            .oppdater_journalpost_tilstand(
                journalpost_id,
                ny_tilstand,
                jp.sikri_id,
                Some(journalpostnummer),
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "journalpost",
                journalpost_id.0,
                envelope.command_id,
                fra_tilstand,
                tilstand_str(ny_tilstand),
                "journalfoer",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(())
    }

    pub(super) async fn avskriv(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), EksekveringFeil> {
        let jp = finn_journalpost(sak, journalpost_id)?;

        let journalpostnummer = jp
            .journalpostnummer
            .ok_or_else(|| EksekveringFeil::blocked("Journalpostnummer mangler for avskriving"))?;

        self.arkiv_gateway
            .avskriv_journalpost(journalpostnummer, "TE")
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        let fra_tilstand = tilstand_str(jp.tilstand);

        self.entity_tilstand_repo
            .oppdater_journalpost_tilstand(
                journalpost_id,
                JournalpostTilstand::Avskrevet,
                jp.sikri_id,
                Some(journalpostnummer),
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "journalpost",
                journalpost_id.0,
                envelope.command_id,
                fra_tilstand,
                "avskrevet",
                "avskriv",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(())
    }
}

fn finn_journalpost(
    sak: &SakMedBarn,
    journalpost_id: SkuffenJournalpostId,
) -> Result<&JournalpostMedDokumenter, EksekveringFeil> {
    sak.journalposter
        .iter()
        .find(|jp| jp.journalpost_id == journalpost_id)
        .ok_or_else(|| {
            EksekveringFeil::recoverable(format!(
                "Fant ikke journalpost {} i sak {}",
                journalpost_id.0, sak.sak_id.0
            ))
        })
}

fn resolve_utsending(jp: &JournalpostMedDokumenter) -> Option<Utsendingsvalg> {
    match jp.journalposttype {
        JournalpostType::Utgaaende => {
            if jp.med_utsending {
                Some(Utsendingsvalg::MedUtsending)
            } else {
                Some(Utsendingsvalg::UtenUtsending)
            }
        }
        _ => None,
    }
}

fn tilstand_str(tilstand: JournalpostTilstand) -> &'static str {
    match tilstand {
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
