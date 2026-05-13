use std::collections::HashSet;

use lib_schemas::skuffen::dokument::Felt;

const SAKSNUMMER_TOKEN: &str = "{{saksnummer}}";
const MAX_TEMPLATE_BYTES: usize = 5 * 1024 * 1024;

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
    #[error("Deklarerte felter kan ikke være tomme")]
    TommeFelter,
    #[error("Saksnummer mangler")]
    ManglerSaksnummer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeltVerdier<'a> {
    pub saksnummer: Option<&'a str>,
}

pub fn er_felter_klare(felter: &[Felt], verdier: &FeltVerdier<'_>) -> bool {
    felter.iter().all(|felt| match felt {
        Felt::Saksnummer => verdier.saksnummer.is_some(),
    })
}

pub fn valider_felter(felter: &[Felt]) -> Result<(), HtmlTemplateFeil> {
    if felter.is_empty() {
        return Err(HtmlTemplateFeil::TommeFelter);
    }

    let mut sett = HashSet::with_capacity(felter.len());
    for felt in felter {
        if !sett.insert(*felt) {
            return Err(HtmlTemplateFeil::DuplikatFelt);
        }
    }

    Ok(())
}

pub fn valider_tokens(html: &[u8], felter: &[Felt]) -> Result<(), HtmlTemplateFeil> {
    let tokens = scan_tokens(html)?;
    valider_felter(felter)?;

    let deklarerte: HashSet<Felt> = felter.iter().copied().collect();
    if tokens.len() > deklarerte.len() {
        return Err(HtmlTemplateFeil::DuplikatToken);
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
    felter: &[Felt],
    verdier: &FeltVerdier<'_>,
) -> Result<Vec<u8>, HtmlTemplateFeil> {
    valider_tokens(html, felter)?;
    if !er_felter_klare(felter, verdier) {
        return Err(HtmlTemplateFeil::ManglerSaksnummer);
    }

    let html = std::str::from_utf8(html).map_err(|_| HtmlTemplateFeil::UgyldigUtf8)?;
    let mut rendered = html.to_string();

    if felter.contains(&Felt::Saksnummer) {
        let saksnummer = verdier
            .saksnummer
            .ok_or(HtmlTemplateFeil::ManglerSaksnummer)?;
        rendered = rendered.replace(SAKSNUMMER_TOKEN, saksnummer);
    }

    Ok(rendered.into_bytes())
}

fn scan_tokens(html: &[u8]) -> Result<HashSet<Felt>, HtmlTemplateFeil> {
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
            "saksnummer" => Felt::Saksnummer,
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
    fn substituerer_saksnummer() {
        let result = substituer_tokens(
            b"<p>{{saksnummer}}</p>",
            &[Felt::Saksnummer],
            &FeltVerdier {
                saksnummer: Some("2026/42"),
            },
        )
        .expect("template should render");

        assert_eq!(result, b"<p>2026/42</p>");
    }

    #[test]
    fn manglende_token_feiler() {
        let err = valider_tokens(b"<p>ingen token</p>", &[Felt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::ManglerToken);
    }

    #[test]
    fn ekstra_token_feiler() {
        let err = valider_tokens(b"{{saksnummer}} {{ukjent}}", &[Felt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::UkjentToken);
    }

    #[test]
    fn duplikate_tokens_feiler() {
        let err =
            valider_tokens(b"{{saksnummer}} {{saksnummer}}", &[Felt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::DuplikatToken);
    }

    #[test]
    fn store_inputs_avvises() {
        let html = vec![b'a'; MAX_TEMPLATE_BYTES + 1];
        let err = valider_tokens(&html, &[Felt::Saksnummer]).unwrap_err();
        assert_eq!(err, HtmlTemplateFeil::ForStor);
    }

    #[test]
    fn readiness_krever_saksnummer() {
        assert!(!er_felter_klare(
            &[Felt::Saksnummer],
            &FeltVerdier { saksnummer: None }
        ));
        assert!(er_felter_klare(
            &[Felt::Saksnummer],
            &FeltVerdier {
                saksnummer: Some("2026/1")
            }
        ));
    }
}
