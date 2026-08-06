//! Skjermings-markup i fritekstfelter (titler).
//!
//! Titler eies av klienten som fri tekst, men skjermings-markup `[ ]` i teksten
//! må stemme med om objektet faktisk er skjermet. Dette er en domeneregel:
//! skjermet tekst uten rettslig skjerming er en skjermingsfeil.

/// Resultatet av en skjermings-markup-sjekk på et fritekstfelt.
#[derive(Debug, PartialEq, Eq)]
pub enum MarkupSjekk {
    Ok,
    /// Feltet inneholder skjermings-markup `[ ]` uten at objektet er skjermet.
    SkjermingKrevesMenMangler,
    /// Feltet har ubalanserte klammer.
    UbalansertKlamme,
}

/// Sjekker at et fritekstfelt (tittel/sakstittel) som inneholder
/// skjermings-markup `[ ]` faktisk er skjermet.
///
/// Regler:
/// - `\[` og `\]` er escapede, litterale klammer og teller ikke som markup.
/// - Uskjermet markup er en feil når `er_skjermet` er `false`.
/// - Uparede klammer avvises.
///
/// Personnavn-markup `|navn|` krever ingenting og påvirker ikke resultatet.
pub fn sjekk_skjerming_markup(tekst: &str, er_skjermet: bool) -> MarkupSjekk {
    let mut har_skjermingsmarkup = false;
    let mut aapne = 0i32;
    let mut chars = tekst.chars();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                // Escapet tegn: hopp over neste (litteral `[` eller `]`).
                chars.next();
            }
            '[' => {
                aapne += 1;
                har_skjermingsmarkup = true;
            }
            ']' => {
                aapne -= 1;
                if aapne < 0 {
                    return MarkupSjekk::UbalansertKlamme;
                }
            }
            _ => {}
        }
    }

    if aapne != 0 {
        return MarkupSjekk::UbalansertKlamme;
    }

    if har_skjermingsmarkup && !er_skjermet {
        return MarkupSjekk::SkjermingKrevesMenMangler;
    }

    MarkupSjekk::Ok
}

/// Sjekker at et korrespondansepart-navn er markup-fritt: merking og skjerming
/// av parter uttrykkes strukturert (parttype + skjerming), ikke i teksten.
pub fn navn_er_markup_fritt(navn: &str) -> bool {
    let mut chars = navn.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' | ']' | '|' => return false,
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ren_tittel_uten_markup_er_ok_uansett() {
        assert_eq!(
            sjekk_skjerming_markup("Vanlig tittel", false),
            MarkupSjekk::Ok
        );
    }

    #[test]
    fn skjermingsmarkup_uten_skjerming_avvises() {
        assert_eq!(
            sjekk_skjerming_markup("Vedtak - [skjermet]", false),
            MarkupSjekk::SkjermingKrevesMenMangler
        );
    }

    #[test]
    fn skjermingsmarkup_med_skjerming_er_ok() {
        assert_eq!(
            sjekk_skjerming_markup("[|Ola Nordmann|] - Vedtak", true),
            MarkupSjekk::Ok
        );
    }

    #[test]
    fn personnavn_markup_alene_krever_ingenting() {
        assert_eq!(
            sjekk_skjerming_markup("|Ola Nordmann| - Vedtak", false),
            MarkupSjekk::Ok
        );
    }

    #[test]
    fn escapet_klamme_er_litteral() {
        assert_eq!(
            sjekk_skjerming_markup("Referanse \\[1\\] i teksten", false),
            MarkupSjekk::Ok
        );
    }

    #[test]
    fn ubalansert_klamme_avvises() {
        assert_eq!(
            sjekk_skjerming_markup("Vedtak - [uferdig", true),
            MarkupSjekk::UbalansertKlamme
        );
        assert_eq!(
            sjekk_skjerming_markup("Vedtak ] rart", true),
            MarkupSjekk::UbalansertKlamme
        );
    }

    #[test]
    fn navn_med_markup_avvises() {
        assert!(!navn_er_markup_fritt("[Ola]"));
        assert!(!navn_er_markup_fritt("|Ola|"));
        assert!(navn_er_markup_fritt("Ola Nordmann"));
    }
}
