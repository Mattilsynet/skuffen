use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId};
use domain::eksekvering::tilstand::{DokumentTilstand, SakMedBarn};
use domain::eksekvering::typer::{EksekveringFeil, EksekveringFeiltype};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use super::{extract_dokument_client_references, EksekverKommandoService};

impl EksekverKommandoService {
    pub(super) async fn legg_til_dokument(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    ) -> Result<(), EksekveringFeil> {
        let jp = sak
            .journalposter
            .iter()
            .find(|jp| jp.journalpost_id == journalpost_id)
            .ok_or_else(|| {
                EksekveringFeil::recoverable(format!(
                    "Fant ikke journalpost {} i sak {}",
                    journalpost_id.0, sak.sak_id.0
                ))
            })?;

        let journalpostnummer = jp.journalpostnummer.ok_or_else(|| {
            EksekveringFeil::blocked("Journalpostnummer mangler for legg_til_dokument")
        })?;

        let dokument_client_reference = self
            .resolve_dokument_client_reference(envelope, dokument_id)
            .await?;

        let resp = match self
            .arkiv_gateway
            .legg_til_vedlegg(envelope, journalpostnummer, vec![dokument_client_reference])
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                let err = self.map_arkiv_feil(err);
                if matches!(err.feiltype, EksekveringFeiltype::Irrecoverable) {
                    let _ = self
                        .entity_tilstand_repo
                        .oppdater_dokument_tilstand(
                            dokument_id,
                            DokumentTilstand::FeiletPermanent,
                            Some(&err.melding),
                        )
                        .await;

                    let _ = self
                        .entity_tilstand_repo
                        .logg_overgang(
                            "dokument",
                            dokument_id.0,
                            envelope.command_id,
                            "ikke_realisert",
                            "feilet_permanent",
                            "legg_til_dokument",
                            Some(&err.melding),
                        )
                        .await;
                }
                return Err(err);
            }
        };

        if let Some(Some(arkiv_id)) = resp.into_iter().next() {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(
                    dokument_client_reference,
                    arkiv_id.to_string(),
                )
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        self.entity_tilstand_repo
            .oppdater_dokument_tilstand(dokument_id, DokumentTilstand::Ok, None)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "dokument",
                dokument_id.0,
                envelope.command_id,
                "ikke_realisert",
                "ok",
                "legg_til_dokument",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(())
    }

    async fn resolve_dokument_client_reference(
        &self,
        envelope: &CommandEnvelope<Command>,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Uuid, EksekveringFeil> {
        let client_references = extract_dokument_client_references(envelope);
        for client_ref in client_references {
            let resolved = self
                .id_mapping
                .hent_dokument_id_fra_mapping(client_ref)
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
            if resolved == Some(dokument_id) {
                return Ok(client_ref);
            }
        }

        Err(EksekveringFeil::recoverable(format!(
            "Fant ikke client_reference for dokument {}",
            dokument_id.0
        )))
    }
}
