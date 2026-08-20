use uuid::Uuid;

use super::*;
use crate::command::{Dekomponeringsinput, DokumentSpesifikasjon, Dokumentkilde};
use crate::eksekvering::html_template::TemplateFelt;
use crate::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, JournalpostType, SakMedBarn, SakTilstand, Saksansvarlig,
};

// ---------------------------------------------------------------------------
// Byggere
// ---------------------------------------------------------------------------

fn sak_id() -> SkuffenSakId {
    SkuffenSakId(Uuid::from_u128(1))
}

fn journalpost_id() -> SkuffenJournalpostId {
    SkuffenJournalpostId(Uuid::from_u128(2))
}

fn dokument_id(n: u128) -> SkuffenDokumentId {
    SkuffenDokumentId(Uuid::from_u128(100 + n))
}

fn operasjon_id(n: u128) -> OperasjonId {
    OperasjonId(Uuid::from_u128(900 + n))
}

fn bytes_spec(n: u128, rekkefolge: u16) -> DokumentSpesifikasjon {
    DokumentSpesifikasjon {
        dokument_id: dokument_id(n),
        rekkefolge,
        kilde: Dokumentkilde::Bytes,
    }
}

fn template_spec(n: u128, rekkefolge: u16) -> DokumentSpesifikasjon {
    DokumentSpesifikasjon {
        dokument_id: dokument_id(n),
        rekkefolge,
        kilde: Dokumentkilde::HtmlTemplate,
    }
}

fn journalpost_input(
    journalposttype: JournalpostType,
    med_utsending: bool,
    dokumenter: Vec<DokumentSpesifikasjon>,
) -> Dekomponeringsinput {
    Dekomponeringsinput::OpprettJournalpost {
        sak_id: sak_id(),
        journalpost_id: journalpost_id(),
        journalposttype,
        med_utsending,
        dokumenter,
    }
}

fn typer(operasjoner: &[Operasjonsspesifikasjon]) -> Vec<Operasjonstype> {
    operasjoner.iter().map(|op| op.operasjonstype).collect()
}

fn dok(n: u128, rekkefolge: u16, tilstand: DokumentTilstand) -> DokumentMedTilstand {
    DokumentMedTilstand {
        dokument_id: dokument_id(n),
        tilstand,
        rekkefolge,
        kilde: DokumentKildeTilstand::Bytes,
    }
}

fn mal(
    n: u128,
    rekkefolge: u16,
    tilstand: DokumentTilstand,
    felter: Vec<TemplateFelt>,
) -> DokumentMedTilstand {
    DokumentMedTilstand {
        dokument_id: dokument_id(n),
        tilstand,
        rekkefolge,
        kilde: DokumentKildeTilstand::HtmlTemplate {
            mal_referanse: Uuid::from_u128(7),
            felter,
            rendered_dokument_referanse: None,
        },
    }
}

fn journalpost(
    tilstand: JournalpostTilstand,
    arkiv_id: Option<&str>,
    journalposttype: JournalpostType,
    dokumenter: Vec<DokumentMedTilstand>,
) -> JournalpostMedDokumenter {
    JournalpostMedDokumenter {
        journalpost_id: journalpost_id(),
        tilstand,
        arkiv_id: arkiv_id.map(str::to_string),
        journalposttype,
        med_utsending: false,
        dokumenter,
    }
}

fn sak(
    tilstand: SakTilstand,
    arkiv_id: Option<&str>,
    journalposter: Vec<JournalpostMedDokumenter>,
) -> SakMedBarn {
    SakMedBarn {
        sak_id: sak_id(),
        tilstand,
        arkiv_id: arkiv_id.map(str::to_string),
        oensket_saksansvarlig: None,
        naavaerende_saksansvarlig: None,
        journalposter,
    }
}

fn op(operasjonstype: Operasjonstype, entitet_id: EntitetId) -> Operasjon {
    Operasjon {
        operasjon_id: operasjon_id(0),
        operasjonstype,
        entitet_id,
        sak_id: sak_id(),
    }
}

fn sak_op_for(operasjonstype: Operasjonstype) -> Operasjon {
    op(operasjonstype, EntitetId::Sak(sak_id()))
}

fn jp_op(operasjonstype: Operasjonstype) -> Operasjon {
    op(operasjonstype, EntitetId::Journalpost(journalpost_id()))
}

fn dok_op(operasjonstype: Operasjonstype, n: u128) -> Operasjon {
    op(operasjonstype, EntitetId::Dokument(dokument_id(n)))
}

// ---------------------------------------------------------------------------
// Dekomponering — alle sju operasjonslistene
// ---------------------------------------------------------------------------

#[test]
fn opprett_sak_dekomponerer_til_en_operasjon() {
    let operasjoner = dekomponer(&Dekomponeringsinput::OpprettSak { sak_id: sak_id() });

    assert_eq!(typer(&operasjoner), vec![Operasjonstype::OpprettSak]);
    assert_eq!(operasjoner[0].entitet_id, EntitetId::Sak(sak_id()));
}

#[test]
fn avslutt_sak_dekomponerer_til_en_operasjon() {
    let operasjoner = dekomponer(&Dekomponeringsinput::AvsluttSak { sak_id: sak_id() });

    assert_eq!(typer(&operasjoner), vec![Operasjonstype::AvsluttSak]);
}

#[test]
fn sett_saksansvarlig_dekomponerer_til_en_operasjon() {
    let operasjoner = dekomponer(&Dekomponeringsinput::SettSaksansvarlig { sak_id: sak_id() });

    assert_eq!(typer(&operasjoner), vec![Operasjonstype::SettSaksansvarlig]);
}

#[test]
fn inngaaende_dekomponerer_til_opprett_vedlegg_journalfor_avskriv() {
    let input = journalpost_input(
        JournalpostType::Inngaende,
        false,
        vec![bytes_spec(0, 0), bytes_spec(1, 1), bytes_spec(2, 2)],
    );

    let operasjoner = dekomponer(&input);

    assert_eq!(
        typer(&operasjoner),
        vec![
            Operasjonstype::OpprettJournalpost,
            Operasjonstype::LeggTilVedlegg,
            Operasjonstype::LeggTilVedlegg,
            Operasjonstype::Journalfor,
            Operasjonstype::Avskriv,
        ]
    );
}

#[test]
fn internt_notat_dekomponerer_uten_avskriv() {
    let input = journalpost_input(JournalpostType::InterntNotat, false, vec![bytes_spec(0, 0)]);

    assert_eq!(
        typer(&dekomponer(&input)),
        vec![
            Operasjonstype::OpprettJournalpost,
            Operasjonstype::Journalfor,
        ]
    );
}

#[test]
fn utgaaende_uten_utsending_dekomponerer_til_sett_ekspedert_og_avvent() {
    let input = journalpost_input(JournalpostType::Utgaaende, false, vec![bytes_spec(0, 0)]);

    assert_eq!(
        typer(&dekomponer(&input)),
        vec![
            Operasjonstype::OpprettJournalpost,
            Operasjonstype::SettEkspedert,
            Operasjonstype::AvventJournalfort,
        ]
    );
}

#[test]
fn utgaaende_med_utsending_dekomponerer_til_klargjor_og_avvent() {
    let input = journalpost_input(JournalpostType::Utgaaende, true, vec![bytes_spec(0, 0)]);

    assert_eq!(
        typer(&dekomponer(&input)),
        vec![
            Operasjonstype::OpprettJournalpost,
            Operasjonstype::KlargjorForEkspedering,
            Operasjonstype::AvventJournalfort,
        ]
    );
}

#[test]
fn skuffen_setter_aldri_j_paa_utgaaende() {
    for med_utsending in [false, true] {
        let input = journalpost_input(
            JournalpostType::Utgaaende,
            med_utsending,
            vec![bytes_spec(0, 0)],
        );

        assert!(
            !typer(&dekomponer(&input)).contains(&Operasjonstype::Journalfor),
            "utgående skal aldri journalføres av Skuffen (SKU-0016 R10)"
        );
    }
}

#[test]
fn html_template_hoveddokument_gir_render_forst() {
    let input = journalpost_input(
        JournalpostType::Utgaaende,
        true,
        vec![template_spec(0, 0), bytes_spec(1, 1)],
    );

    assert_eq!(
        typer(&dekomponer(&input)),
        vec![
            Operasjonstype::RenderDokument,
            Operasjonstype::OpprettJournalpost,
            Operasjonstype::LeggTilVedlegg,
            Operasjonstype::KlargjorForEkspedering,
            Operasjonstype::AvventJournalfort,
        ]
    );
}

#[test]
fn bytes_hoveddokument_gir_ingen_render() {
    let input = journalpost_input(JournalpostType::Inngaende, false, vec![bytes_spec(0, 0)]);

    assert!(!typer(&dekomponer(&input)).contains(&Operasjonstype::RenderDokument));
}

#[test]
fn en_operasjon_per_vedlegg() {
    let dokumenter = (0..6).map(|n| bytes_spec(n as u128, n)).collect();
    let input = journalpost_input(JournalpostType::InterntNotat, false, dokumenter);

    let vedlegg = typer(&dekomponer(&input))
        .into_iter()
        .filter(|t| *t == Operasjonstype::LeggTilVedlegg)
        .count();

    assert_eq!(vedlegg, 5);
}

#[test]
fn dekomponering_er_deterministisk() {
    let input = journalpost_input(
        JournalpostType::Inngaende,
        false,
        vec![bytes_spec(0, 0), bytes_spec(1, 1)],
    );

    assert_eq!(dekomponer(&input), dekomponer(&input));
}

// ---------------------------------------------------------------------------
// muterer_arkivet
// ---------------------------------------------------------------------------

#[test]
fn idempotente_operasjoner_hopper_over_sendt_fasen() {
    // Ren observasjon.
    assert!(!muterer_arkivet(Operasjonstype::AvventJournalfort));
    // Deterministisk nøkkel i object store; kan retryes fritt.
    assert!(!muterer_arkivet(Operasjonstype::RenderDokument));

    for operasjonstype in [
        Operasjonstype::OpprettSak,
        Operasjonstype::OpprettJournalpost,
        Operasjonstype::LeggTilVedlegg,
        Operasjonstype::Journalfor,
        Operasjonstype::SettEkspedert,
        Operasjonstype::KlargjorForEkspedering,
        Operasjonstype::Avskriv,
        Operasjonstype::SettSaksansvarlig,
        Operasjonstype::AvsluttSak,
    ] {
        assert!(muterer_arkivet(operasjonstype), "{operasjonstype:?}");
    }
}

#[test]
fn operasjonstype_koder_er_rundturssikre() {
    for operasjonstype in [
        Operasjonstype::OpprettSak,
        Operasjonstype::RenderDokument,
        Operasjonstype::OpprettJournalpost,
        Operasjonstype::LeggTilVedlegg,
        Operasjonstype::Journalfor,
        Operasjonstype::SettEkspedert,
        Operasjonstype::KlargjorForEkspedering,
        Operasjonstype::AvventJournalfort,
        Operasjonstype::Avskriv,
        Operasjonstype::SettSaksansvarlig,
        Operasjonstype::AvsluttSak,
    ] {
        assert_eq!(
            Operasjonstype::from_code(operasjonstype.as_code()),
            Some(operasjonstype)
        );
    }
}

#[test]
fn operasjonsstatus_koder_er_rundturssikre() {
    for status in [
        Operasjonsstatus::Blokkert,
        Operasjonsstatus::Klar,
        Operasjonsstatus::Kjorer,
        Operasjonsstatus::Sendt,
        Operasjonsstatus::RetryVenter,
        Operasjonsstatus::Ok,
        Operasjonsstatus::Feilet,
        Operasjonsstatus::KreverAvklaring,
    ] {
        assert_eq!(Operasjonsstatus::from_code(status.as_code()), Some(status));
    }
}

// ---------------------------------------------------------------------------
// Prerequisites — OpprettSak
// ---------------------------------------------------------------------------

#[test]
fn opprett_sak_uten_arkiv_id_er_utfor() {
    let facts = sak(SakTilstand::IkkeOpprettet, None, vec![]);

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::OpprettSak), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn opprett_sak_med_arkiv_id_er_allerede_utfort() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::OpprettSak), &facts),
        Beslutning::AlleredeUtfort
    );
}

// ---------------------------------------------------------------------------
// Prerequisites — SettSaksansvarlig
// ---------------------------------------------------------------------------

fn saksansvarlig(id: &str) -> Saksansvarlig {
    Saksansvarlig {
        saksbehandler_id: id.to_string(),
        enhet: "M34600".to_string(),
    }
}

#[test]
fn sett_saksansvarlig_uten_arkiv_id_er_blokkert() {
    let mut facts = sak(SakTilstand::IkkeOpprettet, None, vec![]);
    facts.oensket_saksansvarlig = Some(saksansvarlig("a"));

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::SettSaksansvarlig), &facts),
        Beslutning::Blokkert(BlockedReason::SaksnummerMangler)
    );
}

#[test]
fn sett_saksansvarlig_med_mismatch_er_utfor() {
    let mut facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    facts.oensket_saksansvarlig = Some(saksansvarlig("a"));
    facts.naavaerende_saksansvarlig = Some(saksansvarlig("b"));

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::SettSaksansvarlig), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn sett_saksansvarlig_med_match_er_allerede_utfort() {
    let mut facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    facts.oensket_saksansvarlig = Some(saksansvarlig("a"));
    facts.naavaerende_saksansvarlig = Some(saksansvarlig("a"));

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::SettSaksansvarlig), &facts),
        Beslutning::AlleredeUtfort
    );
}

#[test]
fn sett_saksansvarlig_uten_oensket_er_allerede_utfort() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::SettSaksansvarlig), &facts),
        Beslutning::AlleredeUtfort
    );
}

// ---------------------------------------------------------------------------
// Prerequisites — RenderDokument
// ---------------------------------------------------------------------------

/// Substitusjon er valgfritt. En mal uten deklarerte felter er ren HTML, og
/// skal rendres uten å vente på saksnummer.
#[test]
fn ren_html_uten_substitusjon_rendres_uten_saksnummer() {
    let facts = sak(
        SakTilstand::IkkeOpprettet,
        None,
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Utgaaende,
            vec![mal(0, 0, DokumentTilstand::AvventerRendring, vec![])],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::RenderDokument, 0), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn render_med_saksnummerfelt_blokkeres_uten_arkiv_id() {
    let facts = sak(
        SakTilstand::IkkeOpprettet,
        None,
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Utgaaende,
            vec![mal(
                0,
                0,
                DokumentTilstand::AvventerRendring,
                vec![TemplateFelt::Saksnummer],
            )],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::RenderDokument, 0), &facts),
        Beslutning::Blokkert(BlockedReason::FelterIkkeKlare)
    );
}

#[test]
fn render_med_saksnummerfelt_er_utfor_naar_arkiv_id_finnes() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Utgaaende,
            vec![mal(
                0,
                0,
                DokumentTilstand::AvventerRendring,
                vec![TemplateFelt::Saksnummer],
            )],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::RenderDokument, 0), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn render_paa_bytes_dokument_er_ugyldig() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Utgaaende,
            vec![dok(0, 0, DokumentTilstand::Klar)],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::RenderDokument, 0), &facts),
        Beslutning::Ugyldig(DomainViolation::ForventetHtmlTemplate)
    );
}

#[test]
fn render_av_ferdig_rendret_dokument_er_allerede_utfort() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Utgaaende,
            vec![mal(0, 0, DokumentTilstand::Klar, vec![])],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::RenderDokument, 0), &facts),
        Beslutning::AlleredeUtfort
    );
}

// ---------------------------------------------------------------------------
// Prerequisites — OpprettJournalpost
// ---------------------------------------------------------------------------

#[test]
fn opprett_journalpost_uten_sakens_arkiv_id_er_blokkert() {
    let facts = sak(
        SakTilstand::IkkeOpprettet,
        None,
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Inngaende,
            vec![dok(0, 0, DokumentTilstand::Klar)],
        )],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::OpprettJournalpost), &facts),
        Beslutning::Blokkert(BlockedReason::SaksnummerMangler)
    );
}

#[test]
fn opprett_journalpost_med_urendret_hoveddokument_er_blokkert() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Utgaaende,
            vec![mal(0, 0, DokumentTilstand::AvventerRendring, vec![])],
        )],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::OpprettJournalpost), &facts),
        Beslutning::Blokkert(BlockedReason::HoveddokumentIkkeKlart)
    );
}

#[test]
fn opprett_journalpost_med_klart_hoveddokument_er_utfor() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Inngaende,
            vec![dok(0, 0, DokumentTilstand::Klar)],
        )],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::OpprettJournalpost), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn opprett_journalpost_som_finnes_i_arkivet_er_allerede_utfort() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::Opprettet,
            Some("42"),
            JournalpostType::Inngaende,
            vec![dok(0, 0, DokumentTilstand::Ok)],
        )],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::OpprettJournalpost), &facts),
        Beslutning::AlleredeUtfort
    );
}

#[test]
fn opprett_journalpost_uten_hoveddokument_er_ugyldig() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::IkkeOpprettet,
            None,
            JournalpostType::Inngaende,
            vec![],
        )],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::OpprettJournalpost), &facts),
        Beslutning::Ugyldig(DomainViolation::HoveddokumentMangler)
    );
}

// ---------------------------------------------------------------------------
// Prerequisites — LeggTilVedlegg
// ---------------------------------------------------------------------------

fn journalpost_med_vedlegg(
    tilstand: JournalpostTilstand,
    arkiv_id: Option<&str>,
    vedlegg_tilstand: DokumentTilstand,
) -> SakMedBarn {
    sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            tilstand,
            arkiv_id,
            JournalpostType::Inngaende,
            vec![dok(0, 0, DokumentTilstand::Ok), dok(1, 1, vedlegg_tilstand)],
        )],
    )
}

#[test]
fn legg_til_vedlegg_uten_opprettet_journalpost_er_blokkert() {
    let facts = journalpost_med_vedlegg(
        JournalpostTilstand::IkkeOpprettet,
        None,
        DokumentTilstand::Klar,
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 1), &facts),
        Beslutning::Blokkert(BlockedReason::JournalpostIkkeOpprettet)
    );
}

#[test]
fn legg_til_vedlegg_paa_opprettet_journalpost_er_utfor() {
    let facts = journalpost_med_vedlegg(
        JournalpostTilstand::Opprettet,
        Some("42"),
        DokumentTilstand::Klar,
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 1), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn legg_til_vedlegg_paa_journalfoert_journalpost_er_ugyldig() {
    let facts = journalpost_med_vedlegg(
        JournalpostTilstand::Journalfoert,
        Some("42"),
        DokumentTilstand::Klar,
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 1), &facts),
        Beslutning::Ugyldig(DomainViolation::JournalpostLast)
    );
}

#[test]
fn legg_til_vedlegg_som_allerede_ligger_i_arkivet_er_allerede_utfort() {
    let facts = journalpost_med_vedlegg(
        JournalpostTilstand::Opprettet,
        Some("42"),
        DokumentTilstand::Ok,
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 1), &facts),
        Beslutning::AlleredeUtfort
    );
}

#[test]
fn legg_til_vedlegg_paa_hoveddokumentet_er_ugyldig() {
    let facts = journalpost_med_vedlegg(
        JournalpostTilstand::Opprettet,
        Some("42"),
        DokumentTilstand::Klar,
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 0), &facts),
        Beslutning::Ugyldig(DomainViolation::ForventetVedlegg)
    );
}

#[test]
fn html_template_vedlegg_er_ugyldig() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::Opprettet,
            Some("42"),
            JournalpostType::Inngaende,
            vec![
                dok(0, 0, DokumentTilstand::Ok),
                mal(1, 1, DokumentTilstand::AvventerRendring, vec![]),
            ],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 1), &facts),
        Beslutning::Ugyldig(DomainViolation::HtmlTemplateVedleggIkkeStottet)
    );
}

// ---------------------------------------------------------------------------
// Prerequisites — statusovergangene
// ---------------------------------------------------------------------------

fn journalpost_med_dokumenttilstander(
    tilstand: JournalpostTilstand,
    journalposttype: JournalpostType,
    dokumenter: Vec<DokumentMedTilstand>,
) -> SakMedBarn {
    sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            tilstand,
            Some("42"),
            journalposttype,
            dokumenter,
        )],
    )
}

#[test]
fn journalfor_blokkeres_naar_vedlegg_mangler_i_arkivet() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Inngaende,
        vec![
            dok(0, 0, DokumentTilstand::Ok),
            dok(1, 1, DokumentTilstand::Klar),
        ],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::Journalfor), &facts),
        Beslutning::Blokkert(BlockedReason::DokumenterIkkeKlare)
    );
}

#[test]
fn journalfor_er_utfor_naar_alle_dokumenter_ligger_i_arkivet() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Inngaende,
        vec![
            dok(0, 0, DokumentTilstand::Ok),
            dok(1, 1, DokumentTilstand::Ok),
        ],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::Journalfor), &facts),
        Beslutning::Utfor
    );
}

#[test]
fn journalfor_av_journalfoert_journalpost_er_allerede_utfort() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Journalfoert,
        JournalpostType::Inngaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::Journalfor), &facts),
        Beslutning::AlleredeUtfort
    );
}

#[test]
fn sett_ekspedert_krever_at_alle_dokumenter_ligger_i_arkivet() {
    let blokkert = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Klar)],
    );
    let klar = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::SettEkspedert), &blokkert),
        Beslutning::Blokkert(BlockedReason::DokumenterIkkeKlare)
    );
    assert_eq!(
        vurder(&jp_op(Operasjonstype::SettEkspedert), &klar),
        Beslutning::Utfor
    );
}

#[test]
fn klargjor_for_ekspedering_krever_at_alle_dokumenter_ligger_i_arkivet() {
    let blokkert = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Klar)],
    );
    let klar = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::KlargjorForEkspedering), &blokkert),
        Beslutning::Blokkert(BlockedReason::DokumenterIkkeKlare)
    );
    assert_eq!(
        vurder(&jp_op(Operasjonstype::KlargjorForEkspedering), &klar),
        Beslutning::Utfor
    );
}

#[test]
fn avvent_journalfort_blokkeres_for_ekspedering() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::AvventJournalfort), &facts),
        Beslutning::Blokkert(BlockedReason::JournalpostIkkeEkspedert)
    );
}

#[test]
fn avvent_journalfort_poller_fra_e_og_f() {
    for tilstand in [
        JournalpostTilstand::KlarForEkspedering,
        JournalpostTilstand::Ekspedert,
    ] {
        let facts = journalpost_med_dokumenttilstander(
            tilstand,
            JournalpostType::Utgaaende,
            vec![dok(0, 0, DokumentTilstand::Ok)],
        );

        assert_eq!(
            vurder(&jp_op(Operasjonstype::AvventJournalfort), &facts),
            Beslutning::Utfor,
            "{tilstand:?}"
        );
    }
}

#[test]
fn avvent_journalfort_er_ferdig_ved_journalfoert() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Journalfoert,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::AvventJournalfort), &facts),
        Beslutning::AlleredeUtfort
    );
}

#[test]
fn avskriv_krever_journalfoert_inngaaende() {
    let ikke_journalfoert = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Inngaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );
    let journalfoert = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Journalfoert,
        JournalpostType::Inngaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::Avskriv), &ikke_journalfoert),
        Beslutning::Blokkert(BlockedReason::JournalpostIkkeJournalfort)
    );
    assert_eq!(
        vurder(&jp_op(Operasjonstype::Avskriv), &journalfoert),
        Beslutning::Utfor
    );
}

#[test]
fn avskriv_av_utgaaende_er_ugyldig() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Journalfoert,
        JournalpostType::Utgaaende,
        vec![dok(0, 0, DokumentTilstand::Ok)],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::Avskriv), &facts),
        Beslutning::Ugyldig(DomainViolation::JournalpostTypeMismatch)
    );
}

// ---------------------------------------------------------------------------
// AvsluttSak — søskenregelen (D4)
// ---------------------------------------------------------------------------

fn sammendrag(
    n: u128,
    operasjonstype: Operasjonstype,
    status: Operasjonsstatus,
) -> OperasjonSammendrag {
    OperasjonSammendrag {
        operasjon_id: operasjon_id(n),
        operasjonstype,
        status,
    }
}

#[test]
fn avslutt_sak_blokkeres_naar_sosken_ikke_er_ferdige() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    let sosken = vec![
        sammendrag(1, Operasjonstype::OpprettSak, Operasjonsstatus::Ok),
        sammendrag(2, Operasjonstype::Journalfor, Operasjonsstatus::Blokkert),
    ];

    assert_eq!(
        vurder_avslutt_sak(&sak_op_for(Operasjonstype::AvsluttSak), &facts, &sosken),
        Beslutning::Blokkert(BlockedReason::SoskenIkkeFerdige)
    );
}

#[test]
fn avslutt_sak_blokkeres_av_terminalt_feilet_sosken() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    let sosken = vec![sammendrag(
        1,
        Operasjonstype::LeggTilVedlegg,
        Operasjonsstatus::Feilet,
    )];

    assert_eq!(
        vurder_avslutt_sak(&sak_op_for(Operasjonstype::AvsluttSak), &facts, &sosken),
        Beslutning::Blokkert(BlockedReason::SoskenIkkeFerdige)
    );
}

#[test]
fn avslutt_sak_fanger_sett_saksansvarlig_som_ikke_er_ferdig() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    let sosken = vec![sammendrag(
        1,
        Operasjonstype::SettSaksansvarlig,
        Operasjonsstatus::Klar,
    )];

    assert_eq!(
        vurder_avslutt_sak(&sak_op_for(Operasjonstype::AvsluttSak), &facts, &sosken),
        Beslutning::Blokkert(BlockedReason::SoskenIkkeFerdige)
    );
}

#[test]
fn avslutt_sak_er_utfor_naar_alle_sosken_er_terminalt_ok() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    let sosken = vec![
        sammendrag(1, Operasjonstype::OpprettSak, Operasjonsstatus::Ok),
        sammendrag(2, Operasjonstype::SettSaksansvarlig, Operasjonsstatus::Ok),
    ];

    assert_eq!(
        vurder_avslutt_sak(&sak_op_for(Operasjonstype::AvsluttSak), &facts, &sosken),
        Beslutning::Utfor
    );
}

#[test]
fn avslutt_sak_ignorerer_seg_selv_i_soskenlisten() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);
    let op = sak_op_for(Operasjonstype::AvsluttSak);
    let sosken = vec![OperasjonSammendrag {
        operasjon_id: op.operasjon_id,
        operasjonstype: Operasjonstype::AvsluttSak,
        status: Operasjonsstatus::Kjorer,
    }];

    assert_eq!(vurder_avslutt_sak(&op, &facts, &sosken), Beslutning::Utfor);
}

#[test]
fn avslutt_sak_uten_arkiv_id_er_blokkert() {
    let facts = sak(SakTilstand::IkkeOpprettet, None, vec![]);

    assert_eq!(
        vurder_avslutt_sak(&sak_op_for(Operasjonstype::AvsluttSak), &facts, &[]),
        Beslutning::Blokkert(BlockedReason::SaksnummerMangler)
    );
}

#[test]
fn avslutt_sak_paa_avsluttet_sak_er_allerede_utfort() {
    let facts = sak(SakTilstand::Avsluttet, Some("2026/1"), vec![]);

    assert_eq!(
        vurder_avslutt_sak(&sak_op_for(Operasjonstype::AvsluttSak), &facts, &[]),
        Beslutning::AlleredeUtfort
    );
}

#[test]
fn avslutt_sak_gjennom_vurder_blir_aldri_utfor() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);

    assert_eq!(
        vurder(&sak_op_for(Operasjonstype::AvsluttSak), &facts),
        Beslutning::Blokkert(BlockedReason::SoskenIkkeFerdige)
    );
}

// ---------------------------------------------------------------------------
// Feilisolasjon og typesjekk
// ---------------------------------------------------------------------------

#[test]
fn feil_entitetstype_er_ugyldig() {
    let feilkoblet = op(
        Operasjonstype::OpprettSak,
        EntitetId::Journalpost(journalpost_id()),
    );
    let facts = sak(SakTilstand::IkkeOpprettet, None, vec![]);

    assert_eq!(
        vurder(&feilkoblet, &facts),
        Beslutning::Ugyldig(DomainViolation::EntitetTypeMismatch)
    );
}

#[test]
fn ukjent_journalpost_er_ugyldig() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);

    assert_eq!(
        vurder(&jp_op(Operasjonstype::OpprettJournalpost), &facts),
        Beslutning::Ugyldig(DomainViolation::JournalpostMangler)
    );
}

#[test]
fn ukjent_dokument_er_ugyldig() {
    let facts = sak(SakTilstand::Opprettet, Some("2026/1"), vec![]);

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 9), &facts),
        Beslutning::Ugyldig(DomainViolation::DokumentMangler)
    );
}

// ---------------------------------------------------------------------------
// Stabile koder
// ---------------------------------------------------------------------------

#[test]
fn blocked_reason_har_stabile_koder() {
    assert_eq!(BlockedReason::EntityMissing.as_code(), "entity_missing");
    assert_eq!(
        BlockedReason::SaksnummerMangler.safe_detail(),
        "blocked_reason=saksnummer_mangler"
    );
    assert_eq!(
        BlockedReason::FelterIkkeKlare.safe_detail(),
        "blocked_reason=felter_ikke_klare"
    );
}

#[test]
fn domain_violation_har_stabile_koder() {
    assert_eq!(
        DomainViolation::JournalpostMangler.safe_detail(),
        "invalid_reason=journalpost_mangler"
    );
    assert_eq!(
        DomainViolation::JournalpostLast.safe_detail(),
        "invalid_reason=journalpost_last"
    );
}

#[test]
fn kun_ok_og_feilet_er_terminale() {
    assert!(Operasjonsstatus::Ok.er_terminal());
    assert!(Operasjonsstatus::Feilet.er_terminal());
    for status in [
        Operasjonsstatus::Blokkert,
        Operasjonsstatus::Klar,
        Operasjonsstatus::Kjorer,
        Operasjonsstatus::Sendt,
        Operasjonsstatus::RetryVenter,
        Operasjonsstatus::KreverAvklaring,
    ] {
        assert!(!status.er_terminal(), "{status:?}");
    }
}

// ---------------------------------------------------------------------------
// Feilisolasjon (D12)
// ---------------------------------------------------------------------------

/// Et terminalt feilet vedlegg gjør ikke journalføring *ugyldig* — den forblir
/// **blokkert** til et menneske rydder. Fakta sier bare at dokumentet ikke
/// ligger i arkivet; hvorfor det gikk galt er eksekvering, og bor på
/// operasjonen.
#[test]
fn dokument_som_ikke_ble_arkivert_holder_journalfor_blokkert() {
    let facts = journalpost_med_dokumenttilstander(
        JournalpostTilstand::Opprettet,
        JournalpostType::Inngaende,
        vec![
            dok(0, 0, DokumentTilstand::Ok),
            dok(1, 1, DokumentTilstand::Klar),
        ],
    );

    assert_eq!(
        vurder(&jp_op(Operasjonstype::Journalfor), &facts),
        Beslutning::Blokkert(BlockedReason::DokumenterIkkeKlare),
        "best effort: journalføring venter, den feiler ikke (SKU-0016 R7)"
    );
}

/// Søskenvedlegg skal fortsatt kunne legges til selv om et annet vedlegg ikke
/// kom i arkivet.
#[test]
fn et_uarkivert_vedlegg_stopper_ikke_soskenvedlegg() {
    let facts = sak(
        SakTilstand::Opprettet,
        Some("2026/1"),
        vec![journalpost(
            JournalpostTilstand::Opprettet,
            Some("42"),
            JournalpostType::Inngaende,
            vec![
                dok(0, 0, DokumentTilstand::Ok),
                dok(1, 1, DokumentTilstand::Klar),
                dok(2, 2, DokumentTilstand::Klar),
            ],
        )],
    );

    assert_eq!(
        vurder(&dok_op(Operasjonstype::LeggTilVedlegg, 2), &facts),
        Beslutning::Utfor
    );
}

/// Fakta beskriver arkivet, ikke forsøkene. Sakstilstanden har derfor ingen
/// feilvariant i det hele tatt.
#[test]
fn sakstilstand_har_ingen_feilvariant() {
    for tilstand in [
        SakTilstand::IkkeOpprettet,
        SakTilstand::Opprettet,
        SakTilstand::Avsluttet,
    ] {
        let facts = sak(tilstand, Some("2026/1"), vec![]);
        assert!(
            !matches!(
                vurder(&sak_op_for(Operasjonstype::OpprettSak), &facts),
                Beslutning::Ugyldig(_)
            ),
            "{tilstand:?} skal ikke gi Ugyldig"
        );
    }
}
