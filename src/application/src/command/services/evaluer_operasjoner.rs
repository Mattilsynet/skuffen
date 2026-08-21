use std::sync::Arc;

use anyhow::Result;
use domain::eksekvering::operasjon::{
    Beslutning, Operasjon, Operasjonstype, vurder, vurder_avslutt_sak,
};

use crate::command::ports::{fakta_port::FaktaRepository, operasjon_port::OperasjonRepository};

/// Evalueringspass over blokkerte operasjoner (SKU-0016).
///
/// Leser fakta og flytter `blokkert → klar` for det som nå er kjørbart. Et pass
/// er idempotent og trygt å kjøre så ofte man vil.
///
/// Erstatter v2s hendelsesdrevne wake-up, der en `Done`-melding uten
/// tilhørende operasjon lot blokkerte kommandoer stå for alltid.
pub struct EvaluerOperasjonerService {
    operasjon: Arc<dyn OperasjonRepository>,
    fakta: Arc<dyn FaktaRepository>,
    grense: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Evalueringsresultat {
    pub vurdert: u64,
    /// Ble kjørbare i dette passet.
    pub frigjort: u64,
    pub allerede_utfort: u64,
    pub ugyldig: u64,
}

impl EvaluerOperasjonerService {
    pub fn new(
        operasjon: Arc<dyn OperasjonRepository>,
        fakta: Arc<dyn FaktaRepository>,
        grense: i64,
    ) -> Self {
        Self {
            operasjon,
            fakta,
            grense,
        }
    }

    pub async fn run_evaluation_pass(&self) -> Result<Evalueringsresultat> {
        let blokkerte = self.operasjon.hent_blokkerte(self.grense).await?;
        let mut resultat = Evalueringsresultat::default();

        for op in blokkerte {
            resultat.vurdert += 1;
            match self.beslutt(&op).await? {
                Beslutning::Utfor => {
                    self.operasjon.marker_klar(op.operasjon_id).await?;
                    resultat.frigjort += 1;
                }
                Beslutning::AlleredeUtfort => {
                    self.operasjon
                        .fullfor_ok(
                            op.operasjon_id,
                            0,
                            crate::command::ports::operasjon_port::Faktaoppdatering::Ingen,
                        )
                        .await?;
                    resultat.allerede_utfort += 1;
                }
                Beslutning::Ugyldig(brudd) => {
                    self.operasjon
                        .marker_feilet(op.operasjon_id, 0, &brudd.safe_detail())
                        .await?;
                    resultat.ugyldig += 1;
                }
                Beslutning::Blokkert(grunn) => {
                    // Årsaken oppdateres, men publiseres ikke:
                    // blokkeringsårsak er spørrbar tilstand (D33).
                    self.operasjon
                        .marker_blokkert(op.operasjon_id, None, &grunn.safe_detail())
                        .await?;
                }
            }
        }

        Ok(resultat)
    }

    async fn beslutt(&self, op: &Operasjon) -> Result<Beslutning> {
        let Some(facts) = self.fakta.hent_sak_med_barn(op.sak_id).await? else {
            return Ok(Beslutning::Blokkert(
                domain::eksekvering::operasjon::BlockedReason::EntityMissing,
            ));
        };

        if op.operasjonstype == Operasjonstype::AvsluttSak {
            let sosken = self.operasjon.hent_sammendrag_for_sak(op.sak_id).await?;
            Ok(vurder_avslutt_sak(op, &facts, &sosken))
        } else {
            Ok(vurder(op, &facts))
        }
    }
}
