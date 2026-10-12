//! Operatørbestemt nytt forsøk på stoppede kommandoer.
//!
//! Et nytt forsøk slipper arbeid inn i den ordinære behandlingen igjen. Det
//! avgjør ikke utfallet: validering og executor gjør sine vanlige vurderinger,
//! og samme feil på nytt er et gyldig resultat.

use crate::eksekvering::operasjon::Operasjonsstatus;

/// Hvor en kommando skal inn igjen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gjenopptak {
    /// Ingen operasjoner er materialisert. Kommandoen valideres på nytt fra
    /// originalen; hvorfor den stoppet er uten betydning.
    Revalider,
    /// Operasjonene finnes. De som har stoppet, frigis til executoren.
    FrigiOperasjoner,
    /// Alt er fullført, eller dekomponeringen ga ingen operasjoner.
    IngenEndring,
}

/// Hva som gjøres med én eksisterende operasjon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operasjonsgjenopptak {
    /// `feilet → klar`, forfalt nå.
    GjorKlar,
    /// Venter allerede på et nytt forsøk; forfallet flyttes til nå.
    Fremskynd,
    /// Fullført eller underveis. Pågående arbeid restartes ikke.
    Urort,
}

/// `sendt` er ikke med: den er underveis hos en levende executor, og blir
/// `krever_avklaring` ved recovery hvis executoren døde.
pub fn vurder_command(dekomponert: bool, operasjoner: &[Operasjonsstatus]) -> Gjenopptak {
    if !operasjoner.is_empty() {
        if operasjoner
            .iter()
            .all(|status| *status == Operasjonsstatus::Ok)
        {
            return Gjenopptak::IngenEndring;
        }
        return Gjenopptak::FrigiOperasjoner;
    }
    if dekomponert {
        Gjenopptak::IngenEndring
    } else {
        Gjenopptak::Revalider
    }
}

pub fn vurder_operasjon(status: Operasjonsstatus) -> Operasjonsgjenopptak {
    match status {
        Operasjonsstatus::Feilet => Operasjonsgjenopptak::GjorKlar,
        Operasjonsstatus::RetryVenter | Operasjonsstatus::Blokkert => {
            Operasjonsgjenopptak::Fremskynd
        }
        Operasjonsstatus::Klar
        | Operasjonsstatus::Kjorer
        | Operasjonsstatus::Sendt
        | Operasjonsstatus::Ok
        | Operasjonsstatus::KreverAvklaring => Operasjonsgjenopptak::Urort,
    }
}

/// Et ukjent skriveutfall kan bety at arkivet allerede har effekten. Da
/// avvises hele forespørselen for saken, også når den uavklarte operasjonen
/// tilhører en annen kommando (SKU-0016 R5).
pub fn sak_sperrer_gjenopptak(operasjoner_paa_saken: &[Operasjonsstatus]) -> bool {
    operasjoner_paa_saken.contains(&Operasjonsstatus::KreverAvklaring)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Operasjonsstatus::*;

    #[test]
    fn udekomponert_kommando_uten_operasjoner_revalideres() {
        assert_eq!(vurder_command(false, &[]), Gjenopptak::Revalider);
    }

    #[test]
    fn dekomponert_kommando_uten_operasjoner_har_ingenting_aa_gjenoppta() {
        assert_eq!(vurder_command(true, &[]), Gjenopptak::IngenEndring);
    }

    #[test]
    fn fullfort_kommando_revalideres_ikke() {
        assert_eq!(vurder_command(true, &[Ok, Ok]), Gjenopptak::IngenEndring);
    }

    #[test]
    fn kommando_med_stoppet_arbeid_frigis() {
        for status in [Feilet, RetryVenter, Blokkert, Kjorer, Klar] {
            assert_eq!(
                vurder_command(true, &[Ok, status]),
                Gjenopptak::FrigiOperasjoner,
                "{status:?}"
            );
        }
    }

    #[test]
    fn bare_stoppet_arbeid_endres() {
        assert_eq!(vurder_operasjon(Feilet), Operasjonsgjenopptak::GjorKlar);
        assert_eq!(
            vurder_operasjon(RetryVenter),
            Operasjonsgjenopptak::Fremskynd
        );
        assert_eq!(vurder_operasjon(Blokkert), Operasjonsgjenopptak::Fremskynd);
        for status in [Klar, Kjorer, Sendt, Ok, KreverAvklaring] {
            assert_eq!(
                vurder_operasjon(status),
                Operasjonsgjenopptak::Urort,
                "{status:?}"
            );
        }
    }

    #[test]
    fn uavklart_utfall_paa_saken_sperrer() {
        assert!(sak_sperrer_gjenopptak(&[Ok, Feilet, KreverAvklaring]));
        assert!(!sak_sperrer_gjenopptak(&[Ok, Feilet, Sendt, Kjorer]));
        assert!(!sak_sperrer_gjenopptak(&[]));
    }
}
