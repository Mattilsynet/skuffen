//! Batchkontrollen ved mottak (SKU-0025).
//!
//! Identitetene i en batch må henge sammen før noe skrives: hver kommando har
//! sin egen `command_id`, hver entitet sin egen `client_reference`, og en
//! `client_reference` i `sak_key` må finnes. Kontrollen bruker bare Skuffens
//! egne fakta; arkivet spørres ikke.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::eksekvering::typer::CommandTypeCode;

/// Kommandoens plass i batchen, 0-basert.
pub type Posisjon = usize;

/// Utsnittet av kommandoen som batchkontrollen trenger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batchkommando {
    pub command_id: Uuid,
    pub command_type: CommandTypeCode,
    pub oppretter: Vec<Opprettelse>,
    /// Sakens `client_reference` når kommandoen virker på en eksisterende sak.
    /// `None` for `OpprettSak` og for referanser med arkiv-id.
    pub sak_referanse: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opprettelse {
    pub entitet: Entitetsplass,
    pub client_reference: Uuid,
}

/// Hvor i kommandoen en entitet opprettes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entitetsplass {
    Sak,
    Journalpost,
    /// Posisjon i journalpostens dokumentliste, 0-basert.
    Dokument(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Forekomst {
    pub posisjon: Posisjon,
    pub entitet: Entitetsplass,
}

/// Det Skuffen allerede vet om identitetene batchen nevner.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KjentTilstand {
    pub kommandoer: HashMap<Uuid, CommandTypeCode>,
    pub client_references: HashSet<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Batchfeil {
    CommandIdGjentatt {
        command_id: Uuid,
        posisjoner: Vec<Posisjon>,
    },
    CommandIdBruktAvAnnenKommandotype {
        posisjon: Posisjon,
        command_id: Uuid,
    },
    ClientReferenceGjentatt {
        client_reference: Uuid,
        forekomster: Vec<Forekomst>,
    },
    ClientReferenceFinnesFraFoer {
        forekomst: Forekomst,
        client_reference: Uuid,
    },
    /// `sak_key` viser til en `client_reference` som verken opprettes
    /// tidligere i batchen eller finnes i Skuffen.
    SakReferanseUkjent {
        posisjon: Posisjon,
        client_reference: Uuid,
    },
}

/// Finner alle feil i batchen, ikke bare den første.
///
/// En kommando hvis `command_id` allerede er mottatt med samme type, er en
/// gjentatt innsending. Det den oppretter finnes da fra første innsending, så
/// opprettelsene sjekkes ikke mot `kjent`.
pub fn valider_batch(batch: &[Batchkommando], kjent: &KjentTilstand) -> Result<(), Vec<Batchfeil>> {
    let mut feil = Vec::new();

    for (command_id, posisjoner) in grupper(
        batch
            .iter()
            .enumerate()
            .map(|(posisjon, kommando)| (kommando.command_id, posisjon)),
    ) {
        if posisjoner.len() > 1 {
            feil.push(Batchfeil::CommandIdGjentatt {
                command_id,
                posisjoner,
            });
        }
    }

    let mut gjentatt_innsending = vec![false; batch.len()];
    for (posisjon, kommando) in batch.iter().enumerate() {
        match kjent.kommandoer.get(&kommando.command_id) {
            Some(kjent_type) if *kjent_type == kommando.command_type => {
                gjentatt_innsending[posisjon] = true;
            }
            Some(_) => feil.push(Batchfeil::CommandIdBruktAvAnnenKommandotype {
                posisjon,
                command_id: kommando.command_id,
            }),
            None => {}
        }
    }

    let opprettelser = grupper(batch.iter().enumerate().flat_map(|(posisjon, kommando)| {
        kommando.oppretter.iter().map(move |opprettelse| {
            (
                opprettelse.client_reference,
                Forekomst {
                    posisjon,
                    entitet: opprettelse.entitet,
                },
            )
        })
    }));

    for (client_reference, forekomster) in &opprettelser {
        if forekomster.len() > 1 {
            feil.push(Batchfeil::ClientReferenceGjentatt {
                client_reference: *client_reference,
                forekomster: forekomster.clone(),
            });
        }
        if !kjent.client_references.contains(client_reference) {
            continue;
        }
        for forekomst in forekomster {
            if !gjentatt_innsending[forekomst.posisjon] {
                feil.push(Batchfeil::ClientReferenceFinnesFraFoer {
                    forekomst: *forekomst,
                    client_reference: *client_reference,
                });
            }
        }
    }

    for (posisjon, kommando) in batch.iter().enumerate() {
        let Some(client_reference) = kommando.sak_referanse else {
            continue;
        };
        let opprettet_tidligere = opprettelser.iter().any(|(opprettet, forekomster)| {
            *opprettet == client_reference && forekomster[0].posisjon < posisjon
        });
        if !opprettet_tidligere && !kjent.client_references.contains(&client_reference) {
            feil.push(Batchfeil::SakReferanseUkjent {
                posisjon,
                client_reference,
            });
        }
    }

    if feil.is_empty() { Ok(()) } else { Err(feil) }
}

/// Grupperer på nøkkel og beholder rekkefølgen nøklene først dukket opp i,
/// slik at feilene kommer i samme rekkefølge som i batchen.
fn grupper<V>(par: impl Iterator<Item = (Uuid, V)>) -> Vec<(Uuid, Vec<V>)> {
    let mut indeks: HashMap<Uuid, usize> = HashMap::new();
    let mut grupper: Vec<(Uuid, Vec<V>)> = Vec::new();
    for (nokkel, verdi) in par {
        match indeks.get(&nokkel) {
            Some(&i) => grupper[i].1.push(verdi),
            None => {
                indeks.insert(nokkel, grupper.len());
                grupper.push((nokkel, vec![verdi]));
            }
        }
    }
    grupper
}

#[cfg(test)]
mod tests;
