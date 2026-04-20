use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::tilstand::{SakMedBarn, SakTilstand};
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use super::{extract_sak_client_reference, EksekverKommandoService};

impl EksekverKommandoService {
    pub(super) async fn opprett_sak(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak_id: SkuffenSakId,
    ) -> Result<(), EksekveringFeil> {
        let saksnummer = self
            .arkiv_gateway
            .opprett_sak(envelope)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        if let Some(client_ref) = extract_sak_client_reference(envelope) {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(client_ref, saksnummer.clone())
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        self.entity_tilstand_repo
            .oppdater_sak_tilstand(
                sak_id,
                SakTilstand::Opprettet,
                None,
                Some(&saksnummer),
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "sak",
                sak_id.0,
                envelope.command_id,
                "ikke_realisert",
                "opprettet",
                "opprett_sak",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(())
    }

    pub(super) async fn avslutt_sak(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
    ) -> Result<(), EksekveringFeil> {
        let saksnummer = sak
            .saksnummer
            .as_deref()
            .ok_or_else(|| EksekveringFeil::blocked("Kan ikke avslutte sak: saksnummer mangler"))?;

        self.arkiv_gateway
            .avslutt_sak(saksnummer)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.entity_tilstand_repo
            .oppdater_sak_tilstand(
                sak.sak_id,
                SakTilstand::Avsluttet,
                sak.sikri_id,
                Some(saksnummer),
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "sak",
                sak.sak_id.0,
                envelope.command_id,
                "opprettet",
                "avsluttet",
                "avslutt_sak",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(())
    }
}
