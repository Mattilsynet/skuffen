use std::collections::HashSet;

const SAKSNUMMER_TOKEN: &str = "{{saksnummer}}";
const MAX_TEMPLATE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateFelt {
    Saksnummer,
}

impl TemplateFelt {
    pub fn as_token(self) -> &'static str {
        match self {
            Self::Saksnummer => "saksnummer",
        }
    }

    pub fn token_pattern(self) -> &'static str {
        match self {
            Self::Saksnummer => SAKSNUMMER_TOKEN,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HtmlTemplateFeil {
    #[error("HTML-mal er for stor")]
    ForStor,
    #[error("HTML-mal er ikke gyldig UTF-8")]
    UgyldigUtf8,
    #[error("HTML-mal inneholder ukjent token")]
    UkjentToken,
    #[error("HTML-mal mangler deklarert token")]
    ManglerToken,
    #[error("HTML-mal inneholder duplikat token")]
    DuplikatToken,
    #[error("Deklarerte felter inneholder duplikat")]
    DuplikatFelt,
    #[error("Saksnummer mangler")]
    ManglerSaksnummer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeltVerdier<'a> {
    pub saksnummer: Option<&'a str>,
}

pub fn er_felter_klare(felter: &[TemplateFelt], verdier: &FeltVerdier<'_>) -> bool {
    felter.iter().all(|felt| match felt {
        TemplateFelt::Saksnummer => verdier.saksnummer.is_some(),
    })
}

pub fn valider_felter(felter: &[TemplateFelt]) -> Result<(), HtmlTemplateFeil> {
    let mut sett = HashSet::with_capacity(felter.len());
    for felt in felter {
        if !sett.insert(*felt) {
            return Err(HtmlTemplateFeil::DuplikatFelt);
        }
    }
    Ok(())
}

pub fn valider_tokens(html: &[u8], felter: &[TemplateFelt]) -> Result<(), HtmlTemplateFeil> {
    let tokens = scan_tokens(html)?;
    valider_felter(felter)?;

    let deklarerte: HashSet<TemplateFelt> = felter.iter().copied().collect();

    for token in &tokens {
        if !deklarerte.contains(token) {
            return Err(HtmlTemplateFeil::UkjentToken);
        }
    }

    for felt in &deklarerte {
        if !tokens.contains(felt) {
            return Err(HtmlTemplateFeil::ManglerToken);
        }
    }

    Ok(())
}

pub fn substituer_tokens(
    html: &[u8],
    felter: &[TemplateFelt],
    verdier: &FeltVerdier<'_>,
) -> Result<Vec<u8>, HtmlTemplateFeil> {
    valider_tokens(html, felter)?;

    if felter.is_empty() {
        return Ok(html.to_vec());
    }

    if !er_felter_klare(felter, verdier) {
        return Err(HtmlTemplateFeil::ManglerSaksnummer);
    }

    let html = std::str::from_utf8(html).map_err(|_| HtmlTemplateFeil::UgyldigUtf8)?;
    let mut rendered = html.to_string();

    if felter.contains(&TemplateFelt::Saksnummer) {
        let saksnummer = verdier
            .saksnummer
            .ok_or(HtmlTemplateFeil::ManglerSaksnummer)?;
        rendered = rendered.replace(TemplateFelt::Saksnummer.token_pattern(), saksnummer);
    }

    Ok(rendered.into_bytes())
}

fn scan_tokens(html: &[u8]) -> Result<HashSet<TemplateFelt>, HtmlTemplateFeil> {
    if html.len() > MAX_TEMPLATE_BYTES {
        return Err(HtmlTemplateFeil::ForStor);
    }

    let html = std::str::from_utf8(html).map_err(|_| HtmlTemplateFeil::UgyldigUtf8)?;
    let mut tokens = HashSet::new();
    let mut rest = html;

    while let Some(start) = rest.find("{{") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            return Err(HtmlTemplateFeil::UkjentToken);
        };

        let token = after_start[..end].trim();
        let felt = match token {
            "saksnummer" => TemplateFelt::Saksnummer,
            _ => return Err(HtmlTemplateFeil::UkjentToken),
        };

        if !tokens.insert(felt) {
            return Err(HtmlTemplateFeil::DuplikatToken);
        }

        rest = &after_start[end + 2..];
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_field_token_literals_are_stable() {
        assert_eq!(TemplateFelt::Saksnummer.as_token(), "saksnummer");
        assert_eq!(TemplateFelt::Saksnummer.token_pattern(), "{{saksnummer}}");
    }

    #[test]
    fn substituerer_saksnummer() {
        let result = substituer_tokens(
            b"<p>{{saksnummer}}</p>",
            &[TemplateFelt::Saksnummer],
            &FeltVerdier {
                saksnummer: Some("2026/42"),
            },
        )
        .expect("template should render");

        assert_eq!(result, b"<p>2026/42</p>");
    }

    #[test]
    fn manglende_token_feiler() {
        let err = valider_tokens(b"<p>ingen token</p>", &[TemplateFelt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::ManglerToken);
    }

    #[test]
    fn ekstra_token_feiler() {
        let err =
            valider_tokens(b"{{saksnummer}} {{ukjent}}", &[TemplateFelt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::UkjentToken);
    }

    #[test]
    fn duplikate_tokens_feiler() {
        let err = valider_tokens(
            b"{{saksnummer}} {{saksnummer}}",
            &[TemplateFelt::Saksnummer],
        )
        .unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::DuplikatToken);
    }

    #[test]
    fn store_inputs_avvises() {
        let html = vec![b'a'; MAX_TEMPLATE_BYTES + 1];
        let err = valider_tokens(&html, &[TemplateFelt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::ForStor);
    }

    #[test]
    fn readiness_krever_saksnummer() {
        assert!(!er_felter_klare(
            &[TemplateFelt::Saksnummer],
            &FeltVerdier { saksnummer: None }
        ));
        assert!(er_felter_klare(
            &[TemplateFelt::Saksnummer],
            &FeltVerdier {
                saksnummer: Some("2026/1")
            }
        ));
    }

    #[test]
    fn static_template_ingen_tokens_ingen_felter_ok() {
        let html = b"<p>Dette er en statisk mal uten variabler.</p>";
        let result = substituer_tokens(html, &[], &FeltVerdier { saksnummer: None })
            .expect("static template should succeed");

        assert_eq!(result, html.as_slice());
    }

    #[test]
    fn static_template_validering_ingen_tokens_ingen_felter_ok() {
        let html = b"<p>Statisk innhold</p>";
        let result = valider_tokens(html, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn static_template_med_saksnummer_token_og_tomme_felter_feiler() {
        let html = b"<p>{{saksnummer}}</p>";
        let err = valider_tokens(html, &[]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::UkjentToken);
    }

    #[test]
    fn substituer_tokens_statisk_henter_uforandret() {
        let html = b"<p>Ingen variabler her</p>";
        let result = substituer_tokens(html, &[], &FeltVerdier { saksnummer: None })
            .expect("static substitution should succeed");

        assert_eq!(result, html.as_slice());
    }

    #[test]
    fn er_felter_klare_tomme_felter_er_alltid_klare() {
        assert!(er_felter_klare(&[], &FeltVerdier { saksnummer: None }));
        assert!(er_felter_klare(
            &[],
            &FeltVerdier {
                saksnummer: Some("2026/1")
            }
        ));
    }

    #[test]
    fn valider_felter_tomme_felter_ok() {
        let result = valider_felter(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn valider_felter_duplikat_felt_feiler() {
        let err =
            valider_felter(&[TemplateFelt::Saksnummer, TemplateFelt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::DuplikatFelt);
    }

    #[test]
    fn duplikate_felter_feiler() {
        let err = valider_tokens(
            b"<p>noe</p>",
            &[TemplateFelt::Saksnummer, TemplateFelt::Saksnummer],
        )
        .unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::DuplikatFelt);
    }
}
