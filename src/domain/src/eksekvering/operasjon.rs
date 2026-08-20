//! Operasjonsmodellen (SKU-0016).
//!
//! En operasjon er ett arkivkall med egen identitet. Dekomponering fra kommando
//! til operasjoner skjer én gang og er en ren funksjon av command payload
//! (R2). Avhengigheter utledes fra fakta, ikke fra lagrede kanter (R3).

use crate::command::{Dekomponeringsinput, Dokumentkilde};
use crate::eksekvering::html_template::{FeltVerdier, er_felter_klare};
use crate::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use crate::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, JournalpostType, SakMedBarn, SakTilstand,
};

// ---------------------------------------------------------------------------
// Identitet
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntitetType {
    Sak,
    Journalpost,
    Dokument,
}

impl EntitetType {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Sak => "sak",
            Self::Journalpost => "journalpost",
            Self::Dokument => "dokument",
        }
    }
}

/// Polymorf peker til entiteten operasjonen virker på.
///
/// Databasen garanterer at entiteten finnes, ikke at typen passer
/// operasjonstypen — den regelen lever her (SKU-0016 R12, D28).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntitetId {
    Sak(SkuffenSakId),
    Journalpost(SkuffenJournalpostId),
    Dokument(SkuffenDokumentId),
}

impl EntitetId {
    pub fn entitet_type(self) -> EntitetType {
        match self {
            Self::Sak(_) => EntitetType::Sak,
            Self::Journalpost(_) => EntitetType::Journalpost,
            Self::Dokument(_) => EntitetType::Dokument,
        }
    }

    pub fn as_uuid(self) -> uuid::Uuid {
        match self {
            Self::Sak(id) => id.into(),
            Self::Journalpost(id) => id.into(),
            Self::Dokument(id) => id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperasjonId(pub uuid::Uuid);

impl From<uuid::Uuid> for OperasjonId {
    fn from(value: uuid::Uuid) -> Self {
        Self(value)
    }
}

impl From<OperasjonId> for uuid::Uuid {
    fn from(value: OperasjonId) -> Self {
        value.0
    }
}

// ---------------------------------------------------------------------------
// Operasjonstype
// ---------------------------------------------------------------------------

/// Én operasjonstype er ett API-kall, ikke ett endepunkt. `Journalfor`,
/// `SettEkspedert` og `KlargjorForEkspedering` treffer samme Sikri-endepunkt,
/// men har ulike prerequisites og ulik betydning utad (D22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operasjonstype {
    OpprettSak,
    RenderDokument,
    OpprettJournalpost,
    LeggTilVedlegg,
    Journalfor,
    SettEkspedert,
    KlargjorForEkspedering,
    AvventJournalfort,
    Avskriv,
    SettSaksansvarlig,
    AvsluttSak,
}

impl Operasjonstype {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::OpprettSak => "opprett_sak",
            Self::RenderDokument => "render_dokument",
            Self::OpprettJournalpost => "opprett_journalpost",
            Self::LeggTilVedlegg => "legg_til_vedlegg",
            Self::Journalfor => "journalfor",
            Self::SettEkspedert => "sett_ekspedert",
            Self::KlargjorForEkspedering => "klargjor_for_ekspedering",
            Self::AvventJournalfort => "avvent_journalfort",
            Self::Avskriv => "avskriv",
            Self::SettSaksansvarlig => "sett_saksansvarlig",
            Self::AvsluttSak => "avslutt_sak",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let value = match code {
            "opprett_sak" => Self::OpprettSak,
            "render_dokument" => Self::RenderDokument,
            "opprett_journalpost" => Self::OpprettJournalpost,
            "legg_til_vedlegg" => Self::LeggTilVedlegg,
            "journalfor" => Self::Journalfor,
            "sett_ekspedert" => Self::SettEkspedert,
            "klargjor_for_ekspedering" => Self::KlargjorForEkspedering,
            "avvent_journalfort" => Self::AvventJournalfort,
            "avskriv" => Self::Avskriv,
            "sett_saksansvarlig" => Self::SettSaksansvarlig,
            "avslutt_sak" => Self::AvsluttSak,
            _ => return None,
        };
        Some(value)
    }

    /// Hvilken entitetstype operasjonen forventer å peke på.
    pub fn forventet_entitet_type(self) -> EntitetType {
        match self {
            Self::OpprettSak | Self::SettSaksansvarlig | Self::AvsluttSak => EntitetType::Sak,
            Self::RenderDokument | Self::LeggTilVedlegg => EntitetType::Dokument,
            Self::OpprettJournalpost
            | Self::Journalfor
            | Self::SettEkspedert
            | Self::KlargjorForEkspedering
            | Self::AvventJournalfort
            | Self::Avskriv => EntitetType::Journalpost,
        }
    }
}

/// Styrer om operasjonen må gjennom `sendt`-fasen før utførelse (D7).
///
/// Bare operasjoner som faktisk endrer arkivet trenger at-most-once-grensen.
/// `AvventJournalfort` er en ren observasjon, og `RenderDokument` skriver til
/// object store på en deterministisk nøkkel — begge kan gjentas uten
/// konsekvens og skal derfor kunne retryes fritt i stedet for å havne i
/// `krever_avklaring` etter en crash.
pub fn muterer_arkivet(operasjonstype: Operasjonstype) -> bool {
    !matches!(
        operasjonstype,
        Operasjonstype::AvventJournalfort | Operasjonstype::RenderDokument
    )
}

// ---------------------------------------------------------------------------
// Operasjonsrader
// ---------------------------------------------------------------------------

/// Det dekomponering produserer: én rad som skal skrives til `operasjon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operasjonsspesifikasjon {
    pub operasjonstype: Operasjonstype,
    pub entitet_id: EntitetId,
}

/// En persistert operasjon slik executor og evalueringspasset ser den.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Operasjon {
    pub operasjon_id: OperasjonId,
    pub operasjonstype: Operasjonstype,
    pub entitet_id: EntitetId,
    pub sak_id: SkuffenSakId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operasjonsstatus {
    Blokkert,
    Klar,
    Kjorer,
    Sendt,
    RetryVenter,
    Ok,
    Feilet,
    KreverAvklaring,
}

impl Operasjonsstatus {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Blokkert => "blokkert",
            Self::Klar => "klar",
            Self::Kjorer => "kjorer",
            Self::Sendt => "sendt",
            Self::RetryVenter => "retry_venter",
            Self::Ok => "ok",
            Self::Feilet => "feilet",
            Self::KreverAvklaring => "krever_avklaring",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let value = match code {
            "blokkert" => Self::Blokkert,
            "klar" => Self::Klar,
            "kjorer" => Self::Kjorer,
            "sendt" => Self::Sendt,
            "retry_venter" => Self::RetryVenter,
            "ok" => Self::Ok,
            "feilet" => Self::Feilet,
            "krever_avklaring" => Self::KreverAvklaring,
            _ => return None,
        };
        Some(value)
    }

    /// Terminal betyr at utfallet er avgjort (R8).
    pub fn er_terminal(self) -> bool {
        matches!(self, Self::Ok | Self::Feilet)
    }
}

/// Nok informasjon om en søskenoperasjon til å avgjøre `AvsluttSak`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperasjonSammendrag {
    pub operasjon_id: OperasjonId,
    pub operasjonstype: Operasjonstype,
    pub status: Operasjonsstatus,
}

// ---------------------------------------------------------------------------
// Beslutning
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Beslutning {
    /// Prerequisites er oppfylt; operasjonen kan kjøre nå.
    Utfor,
    /// Prerequisites er ikke oppfylt ennå. Ikke terminalt.
    Blokkert(BlockedReason),
    /// Fakta viser at effekten allerede finnes. Terminalt ok uten arkivkall.
    AlleredeUtfort,
    /// Operasjonen kan aldri utføres. Terminalt feilet.
    Ugyldig(DomainViolation),
}

/// Hvorfor en operasjon ikke er kjørbar ennå. Kodene er stabile og brukes i
/// dashboards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedReason {
    /// Entiteten operasjonen peker på finnes ikke i faktabildet.
    EntityMissing,
    /// Saken har ikke arkiv-id ennå.
    SaksnummerMangler,
    /// HTML-malen mangler verdier den deklarerer.
    FelterIkkeKlare,
    /// Hoveddokumentets innhold er ikke klart.
    HoveddokumentIkkeKlart,
    /// Journalposten finnes ikke i arkivet ennå.
    JournalpostIkkeOpprettet,
    /// Ikke alle dokumenter ligger i arkivet.
    DokumenterIkkeKlare,
    /// Journalposten er ikke satt til `E` eller `F` ennå.
    JournalpostIkkeEkspedert,
    /// Journalposten er ikke journalført ennå.
    JournalpostIkkeJournalfort,
    /// Andre operasjoner på saken er ikke terminalt ok.
    SoskenIkkeFerdige,
    /// Journalpostens fakta matcher ingen kjent kjørbar tilstand.
    JournalpostTilstandUavklart,
}

impl BlockedReason {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::EntityMissing => "entity_missing",
            Self::SaksnummerMangler => "saksnummer_mangler",
            Self::FelterIkkeKlare => "felter_ikke_klare",
            Self::HoveddokumentIkkeKlart => "hoveddokument_ikke_klart",
            Self::JournalpostIkkeOpprettet => "journalpost_ikke_opprettet",
            Self::DokumenterIkkeKlare => "dokumenter_ikke_klare",
            Self::JournalpostIkkeEkspedert => "journalpost_ikke_ekspedert",
            Self::JournalpostIkkeJournalfort => "journalpost_ikke_journalfort",
            Self::SoskenIkkeFerdige => "sosken_ikke_ferdige",
            Self::JournalpostTilstandUavklart => "journalpost_tilstand_uavklart",
        }
    }

    pub fn safe_detail(self) -> String {
        format!("blocked_reason={}", self.as_code())
    }
}

/// Hvorfor en operasjon aldri kan utføres. Terminal feil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainViolation {
    /// Operasjonen peker på en entitet av feil type.
    EntitetTypeMismatch,
    JournalpostMangler,
    DokumentMangler,
    HoveddokumentMangler,
    JournalpostTypeMismatch,
    /// Journalposten er låst; vedlegg kan ikke legges til.
    JournalpostLast,
    /// `RenderDokument` på et dokument som ikke er HTML-mal.
    ForventetHtmlTemplate,
    /// `LeggTilVedlegg` på hoveddokumentet.
    ForventetVedlegg,
    /// HTML-mal som vedlegg støttes ikke (SKU-0005 R9).
    HtmlTemplateVedleggIkkeStottet,
}

impl DomainViolation {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::EntitetTypeMismatch => "entitet_type_mismatch",
            Self::JournalpostMangler => "journalpost_mangler",
            Self::DokumentMangler => "dokument_mangler",
            Self::HoveddokumentMangler => "hoveddokument_mangler",
            Self::JournalpostTypeMismatch => "journalpost_type_mismatch",
            Self::JournalpostLast => "journalpost_last",
            Self::ForventetHtmlTemplate => "forventet_html_template",
            Self::ForventetVedlegg => "forventet_vedlegg",
            Self::HtmlTemplateVedleggIkkeStottet => "html_template_vedlegg_ikke_stottet",
        }
    }

    pub fn safe_detail(self) -> String {
        format!("invalid_reason={}", self.as_code())
    }
}

// ---------------------------------------------------------------------------
// Dekomponering
// ---------------------------------------------------------------------------

/// Ren funksjon av command payload. Kalles én gang, ved innlesing (R2).
pub fn dekomponer(input: &Dekomponeringsinput) -> Vec<Operasjonsspesifikasjon> {
    match input {
        Dekomponeringsinput::OpprettSak { sak_id } => {
            vec![sak_op(Operasjonstype::OpprettSak, *sak_id)]
        }
        Dekomponeringsinput::AvsluttSak { sak_id } => {
            vec![sak_op(Operasjonstype::AvsluttSak, *sak_id)]
        }
        Dekomponeringsinput::SettSaksansvarlig { sak_id } => {
            vec![sak_op(Operasjonstype::SettSaksansvarlig, *sak_id)]
        }
        Dekomponeringsinput::OpprettJournalpost {
            journalpost_id,
            journalposttype,
            med_utsending,
            dokumenter,
            ..
        } => dekomponer_journalpost(
            *journalpost_id,
            *journalposttype,
            *med_utsending,
            dokumenter,
        ),
    }
}

fn dekomponer_journalpost(
    journalpost_id: SkuffenJournalpostId,
    journalposttype: JournalpostType,
    med_utsending: bool,
    dokumenter: &[crate::command::DokumentSpesifikasjon],
) -> Vec<Operasjonsspesifikasjon> {
    let mut operasjoner = Vec::with_capacity(dokumenter.len() + 4);

    // Hoveddokumentet rendres før journalposten opprettes: den rendrede PDF-en
    // er journalpostens hoveddokument (SKU-0005 R2).
    if let Some(hoveddokument) = dokumenter.iter().find(|dok| dok.rekkefolge == 0)
        && matches!(hoveddokument.kilde, Dokumentkilde::HtmlTemplate)
    {
        operasjoner.push(Operasjonsspesifikasjon {
            operasjonstype: Operasjonstype::RenderDokument,
            entitet_id: EntitetId::Dokument(hoveddokument.dokument_id),
        });
    }

    operasjoner.push(journalpost_op(
        Operasjonstype::OpprettJournalpost,
        journalpost_id,
    ));

    // Én operasjon per vedlegg (D5).
    for vedlegg in dokumenter.iter().filter(|dok| dok.rekkefolge > 0) {
        operasjoner.push(Operasjonsspesifikasjon {
            operasjonstype: Operasjonstype::LeggTilVedlegg,
            entitet_id: EntitetId::Dokument(vedlegg.dokument_id),
        });
    }

    match (journalposttype, med_utsending) {
        (JournalpostType::Inngaende, _) => {
            operasjoner.push(journalpost_op(Operasjonstype::Journalfor, journalpost_id));
            operasjoner.push(journalpost_op(Operasjonstype::Avskriv, journalpost_id));
        }
        (JournalpostType::InterntNotat, _) => {
            operasjoner.push(journalpost_op(Operasjonstype::Journalfor, journalpost_id));
        }
        (JournalpostType::Utgaaende, false) => {
            operasjoner.push(journalpost_op(
                Operasjonstype::SettEkspedert,
                journalpost_id,
            ));
            operasjoner.push(journalpost_op(
                Operasjonstype::AvventJournalfort,
                journalpost_id,
            ));
        }
        (JournalpostType::Utgaaende, true) => {
            operasjoner.push(journalpost_op(
                Operasjonstype::KlargjorForEkspedering,
                journalpost_id,
            ));
            operasjoner.push(journalpost_op(
                Operasjonstype::AvventJournalfort,
                journalpost_id,
            ));
        }
    }

    operasjoner
}

fn sak_op(operasjonstype: Operasjonstype, sak_id: SkuffenSakId) -> Operasjonsspesifikasjon {
    Operasjonsspesifikasjon {
        operasjonstype,
        entitet_id: EntitetId::Sak(sak_id),
    }
}

fn journalpost_op(
    operasjonstype: Operasjonstype,
    journalpost_id: SkuffenJournalpostId,
) -> Operasjonsspesifikasjon {
    Operasjonsspesifikasjon {
        operasjonstype,
        entitet_id: EntitetId::Journalpost(journalpost_id),
    }
}

// ---------------------------------------------------------------------------
// Vurdering
// ---------------------------------------------------------------------------

/// Gjelder alle operasjoner unntatt `AvsluttSak`. Fakta alene (R3).
///
/// `AvsluttSak` returnerer alltid `Blokkert` her; den må gå gjennom
/// [`vurder_avslutt_sak`] for å kunne bli `Utfor`.
pub fn vurder(op: &Operasjon, facts: &SakMedBarn) -> Beslutning {
    if op.entitet_id.entitet_type() != op.operasjonstype.forventet_entitet_type() {
        return Beslutning::Ugyldig(DomainViolation::EntitetTypeMismatch);
    }

    match op.operasjonstype {
        Operasjonstype::OpprettSak => vurder_opprett_sak(facts),
        Operasjonstype::SettSaksansvarlig => vurder_sett_saksansvarlig(facts),
        Operasjonstype::AvsluttSak => Beslutning::Blokkert(BlockedReason::SoskenIkkeFerdige),
        Operasjonstype::RenderDokument => med_dokument(op, facts, vurder_render_dokument),
        Operasjonstype::LeggTilVedlegg => med_dokument(op, facts, vurder_legg_til_vedlegg),
        Operasjonstype::OpprettJournalpost => {
            med_journalpost(op, facts, |jp| vurder_opprett_journalpost(jp, facts))
        }
        Operasjonstype::Journalfor => med_journalpost(op, facts, vurder_journalfor),
        Operasjonstype::SettEkspedert => med_journalpost(op, facts, vurder_sett_ekspedert),
        Operasjonstype::KlargjorForEkspedering => {
            med_journalpost(op, facts, vurder_klargjor_for_ekspedering)
        }
        Operasjonstype::AvventJournalfort => med_journalpost(op, facts, vurder_avvent_journalfort),
        Operasjonstype::Avskriv => med_journalpost(op, facts, vurder_avskriv),
    }
}

/// `AvsluttSak` er eneste unntak fra facts-only-regelen (D4). Den krever at
/// alle andre operasjoner på saken er terminalt ok — ikke bare at
/// journalpostene er ferdige.
pub fn vurder_avslutt_sak(
    op: &Operasjon,
    facts: &SakMedBarn,
    sosken: &[OperasjonSammendrag],
) -> Beslutning {
    if op.entitet_id.entitet_type() != EntitetType::Sak {
        return Beslutning::Ugyldig(DomainViolation::EntitetTypeMismatch);
    }
    if facts.tilstand == SakTilstand::Avsluttet {
        return Beslutning::AlleredeUtfort;
    }
    if facts.arkiv_id.is_none() {
        return Beslutning::Blokkert(BlockedReason::SaksnummerMangler);
    }

    let alle_ferdige = sosken
        .iter()
        .filter(|annen| annen.operasjon_id != op.operasjon_id)
        .all(|annen| annen.status == Operasjonsstatus::Ok);

    if alle_ferdige {
        Beslutning::Utfor
    } else {
        Beslutning::Blokkert(BlockedReason::SoskenIkkeFerdige)
    }
}

// --- sak ---

fn vurder_opprett_sak(facts: &SakMedBarn) -> Beslutning {
    if facts.arkiv_id.is_some() {
        return Beslutning::AlleredeUtfort;
    }
    Beslutning::Utfor
}

fn vurder_sett_saksansvarlig(facts: &SakMedBarn) -> Beslutning {
    // Ingen ønsket saksansvarlig er ingenting å gjøre (SKU-0003 R5).
    let Some(oensket) = facts.oensket_saksansvarlig.as_ref() else {
        return Beslutning::AlleredeUtfort;
    };
    if facts.naavaerende_saksansvarlig.as_ref() == Some(oensket) {
        return Beslutning::AlleredeUtfort;
    }
    if facts.arkiv_id.is_none() {
        return Beslutning::Blokkert(BlockedReason::SaksnummerMangler);
    }
    Beslutning::Utfor
}

// --- journalpost ---

fn med_journalpost(
    op: &Operasjon,
    facts: &SakMedBarn,
    f: impl FnOnce(&JournalpostMedDokumenter) -> Beslutning,
) -> Beslutning {
    let EntitetId::Journalpost(id) = op.entitet_id else {
        return Beslutning::Ugyldig(DomainViolation::EntitetTypeMismatch);
    };
    let Some(jp) = facts.journalpost(id) else {
        return Beslutning::Ugyldig(DomainViolation::JournalpostMangler);
    };
    f(jp)
}

fn vurder_opprett_journalpost(jp: &JournalpostMedDokumenter, facts: &SakMedBarn) -> Beslutning {
    if jp.arkiv_id.is_some() || jp.tilstand != JournalpostTilstand::IkkeOpprettet {
        return Beslutning::AlleredeUtfort;
    }
    if facts.arkiv_id.is_none() {
        return Beslutning::Blokkert(BlockedReason::SaksnummerMangler);
    }
    let Some(hoveddokument) = jp.hoveddokument() else {
        return Beslutning::Ugyldig(DomainViolation::HoveddokumentMangler);
    };
    match hoveddokument.tilstand {
        DokumentTilstand::AvventerRendring => {
            Beslutning::Blokkert(BlockedReason::HoveddokumentIkkeKlart)
        }
        DokumentTilstand::Klar | DokumentTilstand::Ok => Beslutning::Utfor,
    }
}

fn vurder_journalfor(jp: &JournalpostMedDokumenter) -> Beslutning {
    if matches!(
        jp.tilstand,
        JournalpostTilstand::Journalfoert | JournalpostTilstand::Avskrevet
    ) {
        return Beslutning::AlleredeUtfort;
    }
    krev_dokumenter_i_arkiv(jp)
}

fn vurder_sett_ekspedert(jp: &JournalpostMedDokumenter) -> Beslutning {
    if matches!(
        jp.tilstand,
        JournalpostTilstand::Ekspedert | JournalpostTilstand::Journalfoert
    ) {
        return Beslutning::AlleredeUtfort;
    }
    krev_dokumenter_i_arkiv(jp)
}

fn vurder_klargjor_for_ekspedering(jp: &JournalpostMedDokumenter) -> Beslutning {
    if matches!(
        jp.tilstand,
        JournalpostTilstand::KlarForEkspedering
            | JournalpostTilstand::Ekspedert
            | JournalpostTilstand::Journalfoert
    ) {
        return Beslutning::AlleredeUtfort;
    }
    krev_dokumenter_i_arkiv(jp)
}

fn vurder_avvent_journalfort(jp: &JournalpostMedDokumenter) -> Beslutning {
    match jp.tilstand {
        JournalpostTilstand::Journalfoert | JournalpostTilstand::Avskrevet => {
            Beslutning::AlleredeUtfort
        }
        JournalpostTilstand::KlarForEkspedering | JournalpostTilstand::Ekspedert => {
            Beslutning::Utfor
        }
        _ => Beslutning::Blokkert(BlockedReason::JournalpostIkkeEkspedert),
    }
}

fn vurder_avskriv(jp: &JournalpostMedDokumenter) -> Beslutning {
    // Utgående journalposter avskrives aldri (D21).
    if jp.journalposttype != JournalpostType::Inngaende {
        return Beslutning::Ugyldig(DomainViolation::JournalpostTypeMismatch);
    }
    match jp.tilstand {
        JournalpostTilstand::Avskrevet => Beslutning::AlleredeUtfort,
        JournalpostTilstand::Journalfoert => Beslutning::Utfor,
        _ => Beslutning::Blokkert(BlockedReason::JournalpostIkkeJournalfort),
    }
}

/// Felles prerequisite for statusovergangene: journalposten finnes i arkivet og
/// alle dokumentene ligger der.
fn krev_dokumenter_i_arkiv(jp: &JournalpostMedDokumenter) -> Beslutning {
    if jp.arkiv_id.is_none() {
        return Beslutning::Blokkert(BlockedReason::JournalpostIkkeOpprettet);
    }
    if jp.dokumenter.is_empty() {
        return Beslutning::Ugyldig(DomainViolation::HoveddokumentMangler);
    }
    if jp
        .dokumenter
        .iter()
        .all(|dok| dok.tilstand == DokumentTilstand::Ok)
    {
        Beslutning::Utfor
    } else {
        Beslutning::Blokkert(BlockedReason::DokumenterIkkeKlare)
    }
}

// --- dokument ---

fn med_dokument(
    op: &Operasjon,
    facts: &SakMedBarn,
    f: impl FnOnce(&JournalpostMedDokumenter, &DokumentMedTilstand, &SakMedBarn) -> Beslutning,
) -> Beslutning {
    let EntitetId::Dokument(id) = op.entitet_id else {
        return Beslutning::Ugyldig(DomainViolation::EntitetTypeMismatch);
    };
    let Some((jp, dok)) = facts.dokument(id) else {
        return Beslutning::Ugyldig(DomainViolation::DokumentMangler);
    };
    f(jp, dok, facts)
}

fn vurder_render_dokument(
    _jp: &JournalpostMedDokumenter,
    dok: &DokumentMedTilstand,
    facts: &SakMedBarn,
) -> Beslutning {
    let DokumentKildeTilstand::HtmlTemplate { felter, .. } = &dok.kilde else {
        return Beslutning::Ugyldig(DomainViolation::ForventetHtmlTemplate);
    };
    // `AvventerRendring` er render-operasjonens readiness-faktum (SKU-0005 R10).
    if dok.tilstand != DokumentTilstand::AvventerRendring {
        return Beslutning::AlleredeUtfort;
    }

    // Ren HTML uten deklarerte felter har ingenting å substituere. Den er klar
    // med én gang og venter ikke på saksnummer
    if felter.is_empty() {
        return Beslutning::Utfor;
    }

    // Har malen deklarert felter, må verdiene finnes før vi rendrer.
    let verdier = FeltVerdier {
        saksnummer: facts.arkiv_id.as_deref(),
    };
    if er_felter_klare(felter, &verdier) {
        Beslutning::Utfor
    } else {
        Beslutning::Blokkert(BlockedReason::FelterIkkeKlare)
    }
}

fn vurder_legg_til_vedlegg(
    jp: &JournalpostMedDokumenter,
    dok: &DokumentMedTilstand,
    _facts: &SakMedBarn,
) -> Beslutning {
    // Hoveddokumentet følger med `OpprettJournalpost`.
    if dok.er_hoveddokument() {
        return Beslutning::Ugyldig(DomainViolation::ForventetVedlegg);
    }
    if matches!(dok.kilde, DokumentKildeTilstand::HtmlTemplate { .. }) {
        return Beslutning::Ugyldig(DomainViolation::HtmlTemplateVedleggIkkeStottet);
    }
    if dok.tilstand == DokumentTilstand::Ok {
        return Beslutning::AlleredeUtfort;
    }
    // En journalført journalpost er låst (D17).
    if matches!(
        jp.tilstand,
        JournalpostTilstand::Journalfoert | JournalpostTilstand::Avskrevet
    ) {
        return Beslutning::Ugyldig(DomainViolation::JournalpostLast);
    }
    if jp.arkiv_id.is_none() {
        return Beslutning::Blokkert(BlockedReason::JournalpostIkkeOpprettet);
    }
    Beslutning::Utfor
}

#[cfg(test)]
mod tests;
