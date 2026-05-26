use crate::eksekvering::html_template::{er_felter_klare, FeltVerdier};
use crate::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use crate::eksekvering::typer::{CommandTypeCode, EksekveringFeil};
use lib_schemas::skuffen::dokument::Felt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostType {
    Inngaende,
    Utgaaende,
    InterntNotat,
}

/// Saksansvarlig (Noark 5 M306) — identifiserer ansvarlig saksbehandler og enhet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saksansvarlig {
    pub saksbehandler_id: String,
    pub enhet: String,
}

// ---------------------------------------------------------------------------
// Tilstander
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SakTilstand {
    IkkeRealisert,
    Opprettet,
    Avsluttet,
    FeiletPermanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostTilstand {
    IkkeRealisert,
    Opprettet,
    DokumenterUnderArbeid,
    KlarForJournalforing,
    VenterPaaUtsending,
    Journalfoert,
    Avskrevet,
    FeiletPermanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DokumentTilstand {
    IkkeRealisert,
    AvventerRendring,
    Ok,
    FeiletPermanent,
}

// ---------------------------------------------------------------------------
// CommandTarget — narrow domain-level command target
// ---------------------------------------------------------------------------

/// Target entity for a command: either a Sak or a specific Journalpost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTarget {
    Sak,
    Journalpost(SkuffenJournalpostId),
}

// ---------------------------------------------------------------------------
// Aggregat-snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SakMedBarn {
    pub sak_id: SkuffenSakId,
    pub tilstand: SakTilstand,
    pub sikri_id: Option<i64>,
    pub saksnummer: Option<String>,
    /// Ønsket saksansvarlig (Noark 5 M306).
    /// Set when a SettSaksansvarlig command is registered.
    pub oensket_saksansvarlig: Option<Saksansvarlig>,
    /// Nåværende saksansvarlig satt i Sikri.
    /// Updated after successful Sikri call.
    pub naavaerende_saksansvarlig: Option<Saksansvarlig>,
    pub journalposter: Vec<JournalpostMedDokumenter>,
}

#[derive(Debug, Clone)]
pub struct JournalpostMedDokumenter {
    pub journalpost_id: SkuffenJournalpostId,
    pub tilstand: JournalpostTilstand,
    pub sikri_id: Option<i64>,
    pub journalpostnummer: Option<i32>,
    pub journalposttype: JournalpostType,
    pub med_utsending: bool,
    pub dokumenter: Vec<DokumentMedTilstand>,
}

#[derive(Debug, Clone)]
pub struct DokumentMedTilstand {
    pub dokument_id: SkuffenDokumentId,
    pub tilstand: DokumentTilstand,
    pub kilde: DokumentKildeTilstand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DokumentKildeTilstand {
    Bytes,
    HtmlTemplate {
        mal_referanse: uuid::Uuid,
        felter: Vec<Felt>,
        rendered_dokument_referanse: Option<uuid::Uuid>,
    },
}

// ---------------------------------------------------------------------------
// Operasjoner
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArkivOperasjon {
    OpprettSak {
        sak_id: SkuffenSakId,
    },
    OpprettJournalpost {
        journalpost_id: SkuffenJournalpostId,
    },
    LeggTilDokument {
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    },
    RenderDokument {
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    },
    Journalfoer {
        journalpost_id: SkuffenJournalpostId,
    },
    Avskriv {
        journalpost_id: SkuffenJournalpostId,
    },
    AvsluttSak {
        sak_id: SkuffenSakId,
    },
    SettSaksansvarlig {
        sak_id: SkuffenSakId,
    },
}

// ---------------------------------------------------------------------------
// CommandStateDecision - new domain planner return type
// ---------------------------------------------------------------------------

/// Reasons why a command is blocked — typed, explicit failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockedReason {
    /// Missing required entity (sak/journalpost)
    EntityMissing,
    /// Missing saksnummer prerequisite
    SaksnummerMangler,
    /// Saksansvarlig mismatch — must be corrected before proceeding
    SaksansvarligIkkeSatt,
    /// Unfinished journalposter block sak closure
    JournalposterIkkeFerdige,
    /// Template missing required field values
    FelterIkkeKlare,
    /// Journalpost exists, but its facts do not match a known executable state.
    JournalpostTilstandUavklart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeupTriggerCategory {
    SakFaktaEndret,
    SaksansvarligOppdatert,
    JournalpostTerminal,
    DokumentFaktaEndret,
    EntityFaktaEndret,
}

impl BlockedReason {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::EntityMissing => "entity_missing",
            Self::SaksnummerMangler => "saksnummer_mangler",
            Self::SaksansvarligIkkeSatt => "saksansvarlig_ikke_satt",
            Self::JournalposterIkkeFerdige => "journalposter_ikke_ferdige",
            Self::FelterIkkeKlare => "felter_ikke_klare",
            Self::JournalpostTilstandUavklart => "journalpost_tilstand_uavklart",
        }
    }

    pub fn trigger_category(self) -> WakeupTriggerCategory {
        match self {
            Self::EntityMissing => WakeupTriggerCategory::EntityFaktaEndret,
            Self::SaksnummerMangler => WakeupTriggerCategory::SakFaktaEndret,
            Self::SaksansvarligIkkeSatt => WakeupTriggerCategory::SaksansvarligOppdatert,
            Self::JournalposterIkkeFerdige => WakeupTriggerCategory::JournalpostTerminal,
            Self::FelterIkkeKlare => WakeupTriggerCategory::DokumentFaktaEndret,
            Self::JournalpostTilstandUavklart => WakeupTriggerCategory::EntityFaktaEndret,
        }
    }

    pub fn safe_detail(self) -> &'static str {
        match self {
            Self::EntityMissing => "blocked_reason=entity_missing",
            Self::SaksnummerMangler => "blocked_reason=saksnummer_mangler",
            Self::SaksansvarligIkkeSatt => "blocked_reason=saksansvarlig_ikke_satt",
            Self::JournalposterIkkeFerdige => "blocked_reason=journalposter_ikke_ferdige",
            Self::FelterIkkeKlare => "blocked_reason=felter_ikke_klare",
            Self::JournalpostTilstandUavklart => "blocked_reason=journalpost_tilstand_uavklart",
        }
    }
}

impl WakeupTriggerCategory {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::SakFaktaEndret => "sak_fakta_endret",
            Self::SaksansvarligOppdatert => "saksansvarlig_oppdatert",
            Self::JournalpostTerminal => "journalpost_terminal",
            Self::DokumentFaktaEndret => "dokument_fakta_endret",
            Self::EntityFaktaEndret => "entity_fakta_endret",
        }
    }
}

/// Domain violation — irrecoverable state that cannot be resolved by retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainViolation {
    /// Journalpost not found for this command type
    JournalpostMangler,
    /// Permanent document failure
    DokumentFeiletPermanent,
    /// Command target mismatch: Sak command given Journalpost target or vice versa
    TargetMismatch,
    /// Journalpost in terminal failure state
    JournalpostFeiletPermanent,
    /// Sak in terminal failure state
    SakFeiletPermanent,
    /// Journalpost type does not match command type
    JournalpostTypeMismatch,
}

impl DomainViolation {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::JournalpostMangler => "journalpost_mangler",
            Self::DokumentFeiletPermanent => "dokument_feilet_permanent",
            Self::TargetMismatch => "target_mismatch",
            Self::JournalpostFeiletPermanent => "journalpost_feilet_permanent",
            Self::SakFeiletPermanent => "sak_feilet_permanent",
            Self::JournalpostTypeMismatch => "journalpost_type_mismatch",
        }
    }

    pub fn safe_detail(self) -> &'static str {
        match self {
            Self::JournalpostMangler => "invalid_reason=journalpost_mangler",
            Self::DokumentFeiletPermanent => "invalid_reason=dokument_feilet_permanent",
            Self::TargetMismatch => "invalid_reason=target_mismatch",
            Self::JournalpostFeiletPermanent => "invalid_reason=journalpost_feilet_permanent",
            Self::SakFeiletPermanent => "invalid_reason=sak_feilet_permanent",
            Self::JournalpostTypeMismatch => "invalid_reason=journalpost_type_mismatch",
        }
    }
}

/// The planner's decision on what to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandStateDecision {
    /// Ready to execute this operation
    Ready(ArkivOperasjon),
    /// Blocked by a known, typed reason
    Blocked(BlockedReason),
    /// Command is complete — nothing more to do
    Done,
    /// Invalid state — irrecoverable domain violation
    Invalid(DomainViolation),
}

// ---------------------------------------------------------------------------
// CommandStateDecision-based planner
// ---------------------------------------------------------------------------

/// Planlegger neste handling basert på command_type, target og sak-tilstand.
///
/// CommandTypeCode er branch-first og styrer planleggingen:
/// - OpprettSak: returnerer Ready(OpprettSak) hvis ingen saksnummer, Done hvis saksnummer finnes
/// - Journalpost commands: krever CommandTarget::Journalpost(id), blokkerer hvis sak mangler
/// - SettSaksansvarlig: blokkerer hvis saksnummer mangler, Ready hvis mismatch, Done hvis match
/// - AvsluttSak: Done hvis allerede avsluttet, Blocked hvis uferdige journalposter/saksansvarlig
///
/// Hver ikke-Ready/ikke-Done sti returnerer eksplisitt BlockedReason eller DomainViolation.
pub fn planlegg_neste_handling(
    command_type: CommandTypeCode,
    target: CommandTarget,
    sak: &SakMedBarn,
) -> CommandStateDecision {
    // Branch first on command type
    match command_type {
        CommandTypeCode::OpprettSak => planlegg_opprett_sak(target, sak),
        CommandTypeCode::OpprettInngaaendeJournalpost
        | CommandTypeCode::OpprettUtgaaendeJournalpost
        | CommandTypeCode::OpprettInterntNotatJournalpost => {
            planlegg_journalpost_command(command_type, target, sak)
        }
        CommandTypeCode::SettSaksansvarlig => planlegg_sett_saksansvarlig(target, sak),
        CommandTypeCode::AvsluttSak => planlegg_avslutt_sak(target, sak),
    }
}

fn planlegg_opprett_sak(target: CommandTarget, sak: &SakMedBarn) -> CommandStateDecision {
    // Sak-level commands require CommandTarget::Sak
    if !matches!(target, CommandTarget::Sak) {
        return CommandStateDecision::Invalid(DomainViolation::TargetMismatch);
    }

    // OpprettSak: no saksnummer -> Ready(OpprettSak), with saksnummer -> Done
    if sak.saksnummer.is_none() {
        return CommandStateDecision::Ready(ArkivOperasjon::OpprettSak { sak_id: sak.sak_id });
    }

    // Saksnummer finnes — opprettelse er ferdig
    CommandStateDecision::Done
}

fn planlegg_journalpost_command(
    command_type: CommandTypeCode,
    target: CommandTarget,
    sak: &SakMedBarn,
) -> CommandStateDecision {
    // Journalpost commands require CommandTarget::Journalpost(id)
    let target_id = match target {
        CommandTarget::Journalpost(id) => id,
        CommandTarget::Sak => {
            return CommandStateDecision::Invalid(DomainViolation::TargetMismatch);
        }
    };

    // Find the target journalpost by ID first (before saksnummer check)
    let jp = match sak
        .journalposter
        .iter()
        .find(|j| j.journalpost_id == target_id)
    {
        Some(jp) => jp,
        None => return CommandStateDecision::Invalid(DomainViolation::JournalpostMangler),
    };

    // Validate journalpost type matches command type (before saksnummer check)
    let expected_type = match command_type {
        CommandTypeCode::OpprettInngaaendeJournalpost => JournalpostType::Inngaende,
        CommandTypeCode::OpprettUtgaaendeJournalpost => JournalpostType::Utgaaende,
        CommandTypeCode::OpprettInterntNotatJournalpost => JournalpostType::InterntNotat,
        _ => unreachable!("caller ensures only journalpost commands reach here"),
    };

    if jp.journalposttype != expected_type {
        return CommandStateDecision::Invalid(DomainViolation::JournalpostTypeMismatch);
    }

    // Missing saksnummer normally blocks journalpost operations, but static HTML
    // hoveddokument rendering has no substitution prerequisites and can run first.
    if sak.saksnummer.is_none() && !kan_planlegge_rendring_uten_saksnummer(jp) {
        return CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler);
    }

    // Check for permanent failure in this journalpost
    if jp.tilstand == JournalpostTilstand::FeiletPermanent {
        return CommandStateDecision::Invalid(DomainViolation::JournalpostFeiletPermanent);
    }

    // Check for permanent document failures only in the target journalpost
    if jp
        .dokumenter
        .iter()
        .any(|d| d.tilstand == DokumentTilstand::FeiletPermanent)
    {
        return CommandStateDecision::Invalid(DomainViolation::DokumentFeiletPermanent);
    }

    // Process only this journalpost's lifecycle
    planlegg_journalpost_lifecycle(jp, sak)
}

fn planlegg_journalpost_lifecycle(
    jp: &JournalpostMedDokumenter,
    sak: &SakMedBarn,
) -> CommandStateDecision {
    // 1. Render template hoveddokument before journalpost creation.
    // Journalpost creation normally requires a hoveddokument; HTML templates must
    // therefore be materialized to a PDF fact before OpprettJournalpost.
    if jp.tilstand == JournalpostTilstand::IkkeRealisert {
        if let Some(hoveddokument) = jp.dokumenter.first() {
            if hoveddokument.tilstand == DokumentTilstand::AvventerRendring {
                if dokument_kan_rendres(hoveddokument, sak.saksnummer.as_deref()) {
                    return CommandStateDecision::Ready(ArkivOperasjon::RenderDokument {
                        journalpost_id: jp.journalpost_id,
                        dokument_id: hoveddokument.dokument_id,
                    });
                } else {
                    return CommandStateDecision::Blocked(BlockedReason::FelterIkkeKlare);
                }
            }
        }

        if sak.saksnummer.is_none() {
            return CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler);
        }

        return CommandStateDecision::Ready(ArkivOperasjon::OpprettJournalpost {
            journalpost_id: jp.journalpost_id,
        });
    }

    // 2. Legg til dokumenter hvis journalpost er opprettet eller under arbeid
    if matches!(
        jp.tilstand,
        JournalpostTilstand::Opprettet | JournalpostTilstand::DokumenterUnderArbeid
    ) {
        // v1 støtter rendring av HTML-template hoveddokument før OpprettJournalpost.
        // HTML-template vedlegg er utenfor v1-scope og feiler terminalt i stedet
        // for å rendres som vedlegg ved et uhell.
        for dok in jp.dokumenter.iter().skip(1) {
            if dok.tilstand == DokumentTilstand::AvventerRendring && dokument_er_html_template(dok)
            {
                return CommandStateDecision::Invalid(DomainViolation::DokumentFeiletPermanent);
            }
        }

        // Sjekk deretter om hoveddokumentet trenger idempotent ferdigstilling av rendring.
        if let Some(dok) = jp.dokumenter.first() {
            if dok.tilstand == DokumentTilstand::AvventerRendring {
                if dokument_kan_rendres(dok, sak.saksnummer.as_deref()) {
                    return CommandStateDecision::Ready(ArkivOperasjon::RenderDokument {
                        journalpost_id: jp.journalpost_id,
                        dokument_id: dok.dokument_id,
                    });
                } else {
                    return CommandStateDecision::Blocked(BlockedReason::FelterIkkeKlare);
                }
            }
        }

        // Then add unrealized documents
        for dok in &jp.dokumenter {
            if dok.tilstand == DokumentTilstand::IkkeRealisert {
                return CommandStateDecision::Ready(ArkivOperasjon::LeggTilDokument {
                    journalpost_id: jp.journalpost_id,
                    dokument_id: dok.dokument_id,
                });
            }
        }
    }

    // 3. Journalfør hvis alle dokumenter er Ok
    if matches!(
        jp.tilstand,
        JournalpostTilstand::Opprettet
            | JournalpostTilstand::DokumenterUnderArbeid
            | JournalpostTilstand::KlarForJournalforing
    ) {
        let alle_dok_ok = !jp.dokumenter.is_empty()
            && jp
                .dokumenter
                .iter()
                .all(|d| d.tilstand == DokumentTilstand::Ok);
        if alle_dok_ok {
            return CommandStateDecision::Ready(ArkivOperasjon::Journalfoer {
                journalpost_id: jp.journalpost_id,
            });
        }
    }

    // 4. Avskriv inngående hvis journalført
    if jp.tilstand == JournalpostTilstand::Journalfoert
        && jp.journalposttype == JournalpostType::Inngaende
    {
        return CommandStateDecision::Ready(ArkivOperasjon::Avskriv {
            journalpost_id: jp.journalpost_id,
        });
    }

    // 5. Terminal tilstander — done for this journalpost
    //    Inngående: Avskrevet
    //    Utgående: Journalført (eller VenterPaaUtsending if med_utsending)
    //    InterntNotat: Journalført
    match jp.journalposttype {
        JournalpostType::Inngaende => {
            if jp.tilstand == JournalpostTilstand::Avskrevet {
                return CommandStateDecision::Done;
            }
        }
        JournalpostType::Utgaaende => {
            if jp.med_utsending {
                if jp.tilstand == JournalpostTilstand::VenterPaaUtsending {
                    return CommandStateDecision::Done;
                }
            } else if jp.tilstand == JournalpostTilstand::Journalfoert {
                return CommandStateDecision::Done;
            }
        }
        JournalpostType::InterntNotat => {
            if jp.tilstand == JournalpostTilstand::Journalfoert {
                return CommandStateDecision::Done;
            }
        }
    }

    // Uklassifisert ikke-terminal journalposttilstand: vent eksplisitt på faktaendring.
    CommandStateDecision::Blocked(BlockedReason::JournalpostTilstandUavklart)
}

fn planlegg_sett_saksansvarlig(target: CommandTarget, sak: &SakMedBarn) -> CommandStateDecision {
    // Sak-level commands require CommandTarget::Sak
    if !matches!(target, CommandTarget::Sak) {
        return CommandStateDecision::Invalid(DomainViolation::TargetMismatch);
    }

    if sak.tilstand == SakTilstand::FeiletPermanent {
        return CommandStateDecision::Invalid(DomainViolation::SakFeiletPermanent);
    }

    // Missing saksnummer -> Blocked
    if sak.saksnummer.is_none() {
        return CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler);
    }

    // No desired saksansvarlig set -> nothing to do (Done)
    let Some(oensket) = &sak.oensket_saksansvarlig else {
        return CommandStateDecision::Done;
    };

    // Mismatch -> Ready to set
    if sak.naavaerende_saksansvarlig.as_ref() != Some(oensket) {
        return CommandStateDecision::Ready(ArkivOperasjon::SettSaksansvarlig {
            sak_id: sak.sak_id,
        });
    }

    // Match -> Done
    CommandStateDecision::Done
}

fn planlegg_avslutt_sak(target: CommandTarget, sak: &SakMedBarn) -> CommandStateDecision {
    // Sak-level commands require CommandTarget::Sak
    if !matches!(target, CommandTarget::Sak) {
        return CommandStateDecision::Invalid(DomainViolation::TargetMismatch);
    }

    // Already closed -> Done
    if sak.tilstand == SakTilstand::Avsluttet {
        return CommandStateDecision::Done;
    }

    if sak.tilstand == SakTilstand::FeiletPermanent {
        return CommandStateDecision::Invalid(DomainViolation::SakFeiletPermanent);
    }

    // Missing sak/saksnummer -> Blocked
    if sak.saksnummer.is_none() {
        return CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler);
    }

    // Saksansvarlig mismatch -> Blocked
    if sak.oensket_saksansvarlig.is_some()
        && sak.oensket_saksansvarlig != sak.naavaerende_saksansvarlig
    {
        return CommandStateDecision::Blocked(BlockedReason::SaksansvarligIkkeSatt);
    }

    // Empty journalposter list -> Ready to close
    if sak.journalposter.is_empty() {
        return CommandStateDecision::Ready(ArkivOperasjon::AvsluttSak { sak_id: sak.sak_id });
    }

    // Check for permanent failures in any journalpost (both journalpost-level and document-level)
    for jp in &sak.journalposter {
        if jp.tilstand == JournalpostTilstand::FeiletPermanent {
            return CommandStateDecision::Invalid(DomainViolation::JournalpostFeiletPermanent);
        }
        if jp
            .dokumenter
            .iter()
            .any(|d| d.tilstand == DokumentTilstand::FeiletPermanent)
        {
            return CommandStateDecision::Invalid(DomainViolation::DokumentFeiletPermanent);
        }
    }

    // Unfinished journalposter -> Blocked
    let alle_terminale = sak.journalposter.iter().all(er_terminal_journalpost);
    if !alle_terminale {
        return CommandStateDecision::Blocked(BlockedReason::JournalposterIkkeFerdige);
    }

    // Prerequisites met -> Ready to close
    CommandStateDecision::Ready(ArkivOperasjon::AvsluttSak { sak_id: sak.sak_id })
}

fn er_terminal_journalpost(jp: &JournalpostMedDokumenter) -> bool {
    match jp.journalposttype {
        JournalpostType::Inngaende => jp.tilstand == JournalpostTilstand::Avskrevet,
        JournalpostType::Utgaaende => {
            if jp.med_utsending {
                jp.tilstand == JournalpostTilstand::VenterPaaUtsending
            } else {
                jp.tilstand == JournalpostTilstand::Journalfoert
            }
        }
        JournalpostType::InterntNotat => jp.tilstand == JournalpostTilstand::Journalfoert,
    }
}

fn kan_planlegge_rendring_uten_saksnummer(jp: &JournalpostMedDokumenter) -> bool {
    if !matches!(
        jp.tilstand,
        JournalpostTilstand::IkkeRealisert
            | JournalpostTilstand::Opprettet
            | JournalpostTilstand::DokumenterUnderArbeid
    ) {
        return false;
    }

    jp.dokumenter.first().is_some_and(|dok| {
        dok.tilstand == DokumentTilstand::AvventerRendring && dokument_kan_rendres(dok, None)
    })
}

fn dokument_kan_rendres(dok: &DokumentMedTilstand, saksnummer: Option<&str>) -> bool {
    match &dok.kilde {
        DokumentKildeTilstand::HtmlTemplate {
            felter,
            rendered_dokument_referanse: _,
            mal_referanse: _,
        } => er_felter_klare(felter, &FeltVerdier { saksnummer }),
        DokumentKildeTilstand::Bytes => false,
    }
}

fn dokument_er_html_template(dok: &DokumentMedTilstand) -> bool {
    matches!(dok.kilde, DokumentKildeTilstand::HtmlTemplate { .. })
}

// ---------------------------------------------------------------------------
// Compatibility wrapper for application migration
// ---------------------------------------------------------------------------

/// Compatibility wrapper: maps CommandStateDecision to old Result<Option<ArkivOperasjon>, EksekveringFeil>
/// - Ready -> Ok(Some(op))
/// - Done -> Ok(None)
/// - Blocked -> Ok(None) (retryable)
/// - Invalid -> Err(irrecoverable)
#[deprecated(
    since = "1.0.0",
    note = "Use planlegg_neste_handling which returns CommandStateDecision"
)]
pub fn neste_handling(
    command_type: CommandTypeCode,
    target: CommandTarget,
    sak: &SakMedBarn,
) -> Result<Option<ArkivOperasjon>, EksekveringFeil> {
    match planlegg_neste_handling(command_type, target, sak) {
        CommandStateDecision::Ready(op) => Ok(Some(op)),
        CommandStateDecision::Done => Ok(None),
        CommandStateDecision::Blocked(_) => Ok(None),
        CommandStateDecision::Invalid(violation) => Err(EksekveringFeil::irrecoverable(
            violation.safe_detail().to_string(),
        )),
    }
}

// ---------------------------------------------------------------------------
// Tester
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sak_id() -> SkuffenSakId {
        SkuffenSakId(Uuid::new_v4())
    }

    fn jp_id() -> SkuffenJournalpostId {
        SkuffenJournalpostId(Uuid::new_v4())
    }

    fn dok_id() -> SkuffenDokumentId {
        SkuffenDokumentId(Uuid::new_v4())
    }

    fn enkel_sak(tilstand: SakTilstand) -> SakMedBarn {
        SakMedBarn {
            sak_id: sak_id(),
            tilstand,
            sikri_id: None,
            saksnummer: None,
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![],
        }
    }

    fn opprettet_sak_med_saksnummer(journalposter: Vec<JournalpostMedDokumenter>) -> SakMedBarn {
        SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(1),
            saksnummer: Some("2025/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter,
        }
    }

    fn lag_journalpost(
        id: SkuffenJournalpostId,
        tilstand: JournalpostTilstand,
        jptype: JournalpostType,
        med_utsending: bool,
        dokumenter: Vec<DokumentMedTilstand>,
    ) -> JournalpostMedDokumenter {
        JournalpostMedDokumenter {
            journalpost_id: id,
            tilstand,
            sikri_id: Some(100),
            journalpostnummer: Some(1),
            journalposttype: jptype,
            med_utsending,
            dokumenter,
        }
    }

    fn dok(tilstand: DokumentTilstand) -> DokumentMedTilstand {
        DokumentMedTilstand {
            dokument_id: dok_id(),
            tilstand,
            kilde: DokumentKildeTilstand::Bytes,
        }
    }

    fn template_dok(tilstand: DokumentTilstand, felter: Vec<Felt>) -> DokumentMedTilstand {
        DokumentMedTilstand {
            dokument_id: dok_id(),
            tilstand,
            kilde: DokumentKildeTilstand::HtmlTemplate {
                mal_referanse: uuid::Uuid::new_v4(),
                felter,
                rendered_dokument_referanse: None,
            },
        }
    }

    // =========================================================================
    // OpprettSak tests
    // =========================================================================

    #[test]
    fn opprett_sak_uten_saksnummer_gir_ready() {
        let sak = enkel_sak(SakTilstand::IkkeRealisert);
        let decision =
            planlegg_neste_handling(CommandTypeCode::OpprettSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::OpprettSak { .. })
        ));
    }

    #[test]
    fn opprett_sak_med_saksnummer_gir_done() {
        let mut sak = enkel_sak(SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        let decision =
            planlegg_neste_handling(CommandTypeCode::OpprettSak, CommandTarget::Sak, &sak);
        assert!(matches!(decision, CommandStateDecision::Done));
    }

    #[test]
    fn blocked_reason_har_stabil_kode_og_triggerkategori() {
        assert_eq!(
            BlockedReason::SaksnummerMangler.as_code(),
            "saksnummer_mangler"
        );
        assert_eq!(
            BlockedReason::SaksnummerMangler
                .trigger_category()
                .as_code(),
            "sak_fakta_endret"
        );
        assert_eq!(
            BlockedReason::JournalposterIkkeFerdige
                .trigger_category()
                .as_code(),
            "journalpost_terminal"
        );
        assert_eq!(
            BlockedReason::SaksansvarligIkkeSatt
                .trigger_category()
                .as_code(),
            "saksansvarlig_oppdatert"
        );
        assert_eq!(
            BlockedReason::FelterIkkeKlare.trigger_category().as_code(),
            "dokument_fakta_endret"
        );
        assert_eq!(
            BlockedReason::JournalpostTilstandUavklart.as_code(),
            "journalpost_tilstand_uavklart"
        );
        assert_eq!(
            BlockedReason::JournalpostTilstandUavklart
                .trigger_category()
                .as_code(),
            "entity_fakta_endret"
        );
    }

    #[test]
    fn domain_violation_har_stabil_kode() {
        assert_eq!(DomainViolation::TargetMismatch.as_code(), "target_mismatch");
        assert_eq!(
            DomainViolation::TargetMismatch.safe_detail(),
            "invalid_reason=target_mismatch"
        );
    }

    #[test]
    fn opprett_sak_aldri_returnerer_sett_saksansvarlig_eller_journalpost_eller_avslutt() {
        let sak = enkel_sak(SakTilstand::IkkeRealisert);
        let decision =
            planlegg_neste_handling(CommandTypeCode::OpprettSak, CommandTarget::Sak, &sak);
        if let CommandStateDecision::Ready(op) = decision {
            assert!(matches!(op, ArkivOperasjon::OpprettSak { .. }));
        }
    }

    #[test]
    fn opprett_sak_med_journalpost_target_gir_invalid() {
        let sak = enkel_sak(SakTilstand::IkkeRealisert);
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettSak,
            CommandTarget::Journalpost(jp_id()),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::TargetMismatch)
        ));
    }

    // =========================================================================
    // Journalpost command tests
    // =========================================================================

    #[test]
    fn journalpost_command_uten_saksnummer_gir_blocked() {
        // Target must exist and match type before saksnummer check applies
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::Inngaende,
            false,
            vec![],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.saksnummer = None; // Remove saksnummer to trigger Blocked

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler)
        ));
    }

    #[test]
    fn journalpost_command_aldri_returnerer_sett_saksansvarlig() {
        let mut sak = opprettet_sak_med_saksnummer(vec![]);
        sak.oensket_saksansvarlig = Some(Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        });
        sak.naavaerende_saksansvarlig = None;

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id()),
            &sak,
        );
        match decision {
            CommandStateDecision::Ready(op) => {
                assert!(!matches!(op, ArkivOperasjon::SettSaksansvarlig { .. }));
            }
            CommandStateDecision::Done
            | CommandStateDecision::Blocked(_)
            | CommandStateDecision::Invalid(_) => {}
        }
    }

    #[test]
    fn journalpost_command_aldri_returnerer_avslutt_sak() {
        let sak = opprettet_sak_med_saksnummer(vec![]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id()),
            &sak,
        );
        match decision {
            CommandStateDecision::Ready(op) => {
                assert!(!matches!(op, ArkivOperasjon::AvsluttSak { .. }));
            }
            CommandStateDecision::Done
            | CommandStateDecision::Blocked(_)
            | CommandStateDecision::Invalid(_) => {}
        }
    }

    #[test]
    fn journalpost_command_kun_for_matching_type() {
        let jp_id_1 = jp_id();
        let jp_id_2 = jp_id();
        let inngaaende_jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Inngaende,
            false,
            vec![],
        );
        let utgaaende_jp = lag_journalpost(
            jp_id_2,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Utgaaende,
            false,
            vec![],
        );

        let sak = SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(1),
            saksnummer: Some("2025/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![inngaaende_jp, utgaaende_jp],
        };

        // OpprettInngaaendeJournalpost should only see inngående journalpost
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        match decision {
            CommandStateDecision::Ready(ArkivOperasjon::OpprettJournalpost { journalpost_id }) => {
                // Should be the inngående one, not utgående
                assert!(sak
                    .journalposter
                    .iter()
                    .find(|jp| jp.journalpost_id == journalpost_id)
                    .map(|jp| jp.journalposttype == JournalpostType::Inngaende)
                    .unwrap_or(false));
            }
            _ => panic!("Expected Ready for inngående journalpost"),
        }
    }

    #[test]
    fn journalpost_command_uten_matching_journalpost_gir_invalid() {
        let jp_id_1 = jp_id();
        // Only utgående journalpost exists
        let utgaaende_jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Utgaaende,
            false,
            vec![],
        );

        let sak = SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(1),
            saksnummer: Some("2025/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![utgaaende_jp],
        };

        // OpprettInngaaendeJournalpost with non-existent id
        let missing_id = jp_id();
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(missing_id),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::JournalpostMangler)
        ));
    }

    #[test]
    fn journalpost_command_med_sak_target_gir_invalid() {
        let jp = lag_journalpost(
            jp_id(),
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Inngaende,
            false,
            vec![],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Sak,
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::TargetMismatch)
        ));
    }

    #[test]
    fn journalpost_command_type_mismatch_gir_invalid() {
        let jp_id_1 = jp_id();
        // Inngående journalpost exists
        let inngaaende_jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Inngaende,
            false,
            vec![],
        );

        let sak = SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(1),
            saksnummer: Some("2025/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![inngaaende_jp],
        };

        // OpprettUtgaaendeJournalpost targeting inngående journalpost
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::JournalpostTypeMismatch)
        ));
    }

    #[test]
    fn journalpost_feilet_permanent_gir_invalid() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::FeiletPermanent,
            JournalpostType::Inngaende,
            false,
            vec![],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::JournalpostFeiletPermanent)
        ));
    }

    // =========================================================================
    // Sibling isolation tests
    // =========================================================================

    #[test]
    fn same_type_sibling_isolation() {
        // Two journalposts of the same type, target is the second one
        let jp_id_1 = jp_id();
        let jp_id_2 = jp_id();
        let jp_1 = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let jp_2 = lag_journalpost(
            jp_id_2,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Inngaende,
            false,
            vec![],
        );

        let sak = opprettet_sak_med_saksnummer(vec![jp_1, jp_2]);

        // Target the second journalpost
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_2),
            &sak,
        );
        match decision {
            CommandStateDecision::Ready(ArkivOperasjon::OpprettJournalpost { journalpost_id }) => {
                assert_eq!(journalpost_id, jp_id_2);
            }
            _ => panic!("Expected Ready for target journalpost"),
        }
    }

    #[test]
    fn sibling_failed_document_does_not_invalidate_target() {
        // Target journalpost has actionable state, sibling has FeiletPermanent document
        let jp_id_1 = jp_id();
        let jp_id_2 = jp_id();
        let jp_sibling = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::FeiletPermanent)],
        );
        let jp_target = lag_journalpost(
            jp_id_2,
            JournalpostTilstand::Opprettet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );

        let sak = opprettet_sak_med_saksnummer(vec![jp_sibling, jp_target]);

        // Target the second journalpost (which has Ok document)
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_2),
            &sak,
        );
        // Should return action for target, not Invalid due to sibling
        match decision {
            CommandStateDecision::Ready(ArkivOperasjon::Journalfoer { journalpost_id }) => {
                assert_eq!(journalpost_id, jp_id_2);
            }
            _ => panic!("Expected Ready for target journalpost, got {:?}", decision),
        }
    }

    // =========================================================================
    // SettSaksansvarlig tests
    // =========================================================================

    #[test]
    fn sett_saksansvarlig_uten_saksnummer_gir_blocked() {
        let sak = enkel_sak(SakTilstand::Opprettet);
        let mut sak = sak;
        sak.oensket_saksansvarlig = Some(Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        });

        let decision =
            planlegg_neste_handling(CommandTypeCode::SettSaksansvarlig, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler)
        ));
    }

    #[test]
    fn sett_saksansvarlig_med_mismatch_gir_ready() {
        let mut sak = enkel_sak(SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = Some(Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        });
        sak.naavaerende_saksansvarlig = None;

        let decision =
            planlegg_neste_handling(CommandTypeCode::SettSaksansvarlig, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::SettSaksansvarlig { .. })
        ));
    }

    #[test]
    fn sett_saksansvarlig_med_match_gir_done() {
        let saksansvarlig = Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        };
        let mut sak = enkel_sak(SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = Some(saksansvarlig.clone());
        sak.naavaerende_saksansvarlig = Some(saksansvarlig);

        let decision =
            planlegg_neste_handling(CommandTypeCode::SettSaksansvarlig, CommandTarget::Sak, &sak);
        assert!(matches!(decision, CommandStateDecision::Done));
    }

    #[test]
    fn sett_saksansvarlig_aldri_returnerer_journalpost_eller_avslutt_ops() {
        let mut sak = enkel_sak(SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = Some(Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        });
        sak.naavaerende_saksansvarlig = None;

        let decision =
            planlegg_neste_handling(CommandTypeCode::SettSaksansvarlig, CommandTarget::Sak, &sak);
        if let CommandStateDecision::Ready(op) = decision {
            assert!(matches!(op, ArkivOperasjon::SettSaksansvarlig { .. }));
        }
    }

    #[test]
    fn sett_saksansvarlig_med_journalpost_target_gir_invalid() {
        let sak = enkel_sak(SakTilstand::Opprettet);
        let decision = planlegg_neste_handling(
            CommandTypeCode::SettSaksansvarlig,
            CommandTarget::Journalpost(jp_id()),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::TargetMismatch)
        ));
    }

    #[test]
    fn sett_saksansvarlig_sak_feilet_permanent_gir_invalid() {
        let mut sak = enkel_sak(SakTilstand::FeiletPermanent);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = Some(Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        });

        let decision =
            planlegg_neste_handling(CommandTypeCode::SettSaksansvarlig, CommandTarget::Sak, &sak);

        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::SakFeiletPermanent)
        ));
    }

    // =========================================================================
    // AvsluttSak tests
    // =========================================================================

    #[test]
    fn avslutt_sak_allerede_avsluttet_gir_done() {
        let mut sak = enkel_sak(SakTilstand::Avsluttet);
        sak.saksnummer = Some("2025/1".to_string());

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(decision, CommandStateDecision::Done));
    }

    #[test]
    fn avslutt_sak_sak_feilet_permanent_gir_invalid() {
        let mut sak = enkel_sak(SakTilstand::FeiletPermanent);
        sak.saksnummer = Some("2025/1".to_string());

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::SakFeiletPermanent)
        ));
    }

    #[test]
    fn avslutt_sak_uten_saksnummer_gir_blocked() {
        let sak = enkel_sak(SakTilstand::Opprettet);

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler)
        ));
    }

    #[test]
    fn avslutt_sak_uferdige_journalposter_gir_blocked() {
        let jp = lag_journalpost(
            jp_id(),
            JournalpostTilstand::Opprettet,
            JournalpostType::InterntNotat,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::JournalposterIkkeFerdige)
        ));
    }

    #[test]
    fn avslutt_sak_saksansvarlig_ikke_satt_gir_blocked() {
        let mut sak = enkel_sak(SakTilstand::Opprettet);
        sak.saksnummer = Some("2025/1".to_string());
        sak.oensket_saksansvarlig = Some(Saksansvarlig {
            saksbehandler_id: "Z123".to_string(),
            enhet: "42".to_string(),
        });
        sak.naavaerende_saksansvarlig = None;

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksansvarligIkkeSatt)
        ));
    }

    #[test]
    fn avslutt_sak_prerequisites_met_gir_ready() {
        let jp_inn = lag_journalpost(
            jp_id(),
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp_inn]);

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::AvsluttSak { .. })
        ));
    }

    #[test]
    fn avslutt_sak_aldri_returnerer_journalpost_eller_saksansvarlig_ops() {
        let jp = lag_journalpost(
            jp_id(),
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        if let CommandStateDecision::Ready(op) = decision {
            assert!(matches!(op, ArkivOperasjon::AvsluttSak { .. }));
        }
    }

    #[test]
    fn avslutt_sak_med_journalpost_target_gir_invalid() {
        let sak = enkel_sak(SakTilstand::Opprettet);
        let decision = planlegg_neste_handling(
            CommandTypeCode::AvsluttSak,
            CommandTarget::Journalpost(jp_id()),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::TargetMismatch)
        ));
    }

    // =========================================================================
    // BlockedReason/DomainViolation classification tests
    // =========================================================================

    #[test]
    fn html_template_venter_paa_saksnummer_gir_blocked_saksnummer_mangler() {
        // When saksnummer is None, journalpost commands return SaksnummerMangler first
        // (guard clause takes precedence over template field checks)
        // Target journalpost must exist and match type for this test
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.saksnummer = None;

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler)
        ));
    }

    #[test]
    fn statisk_html_template_uten_felter_rendres_uten_saksnummer() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(DokumentTilstand::AvventerRendring, vec![])],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.saksnummer = None;

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::RenderDokument { .. })
        ));
    }

    #[test]
    fn html_template_med_saksnummer_felt_blockerer_uten_saksnummer() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.saksnummer = None;

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler)
        ));
    }

    #[test]
    fn statisk_html_template_ok_blockerer_opprett_journalpost_uten_saksnummer() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(DokumentTilstand::Ok, vec![])],
        );
        let mut sak = opprettet_sak_med_saksnummer(vec![jp]);
        sak.saksnummer = None;

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::SaksnummerMangler)
        ));
    }

    #[test]
    fn html_template_rendres_naar_saksnummer_eksisterer() {
        // When saksnummer exists and template needs saksnummer, rendering is ready
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::RenderDokument { .. })
        ));
    }

    #[test]
    fn html_template_rendres_for_journalpost_opprettes() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::InterntNotat,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::RenderDokument { .. })
        ));
    }

    #[test]
    fn html_template_med_rendered_referanse_rendres_ferdig_paa_retry() {
        let jp_id_1 = jp_id();
        let mut dokument = template_dok(DokumentTilstand::AvventerRendring, vec![Felt::Saksnummer]);
        if let DokumentKildeTilstand::HtmlTemplate {
            rendered_dokument_referanse,
            ..
        } = &mut dokument.kilde
        {
            *rendered_dokument_referanse = Some(Uuid::new_v4());
        }
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::InterntNotat,
            false,
            vec![dokument],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::RenderDokument { .. })
        ));
    }

    #[test]
    fn html_template_etter_rendering_oppretter_journalpost() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::InterntNotat,
            false,
            vec![template_dok(DokumentTilstand::Ok, vec![Felt::Saksnummer])],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::OpprettJournalpost { .. })
        ));
    }

    #[test]
    fn ikke_hoveddokument_avventer_rendring_blockerer_ikke_opprett_journalpost() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::InterntNotat,
            false,
            vec![
                dok(DokumentTilstand::Ok),
                template_dok(DokumentTilstand::AvventerRendring, vec![Felt::Saksnummer]),
            ],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::OpprettJournalpost { .. })
        ));
    }

    #[test]
    fn html_template_vedlegg_etter_journalpostopprettelse_feiler_permanent() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::InterntNotat,
            false,
            vec![
                dok(DokumentTilstand::Ok),
                template_dok(DokumentTilstand::AvventerRendring, vec![Felt::Saksnummer]),
            ],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::DokumentFeiletPermanent)
        ));
    }

    #[test]
    fn uklassifisert_journalposttilstand_gir_presis_blocked_reason() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::KlarForJournalforing,
            JournalpostType::Utgaaende,
            false,
            vec![template_dok(
                DokumentTilstand::AvventerRendring,
                vec![Felt::Saksnummer],
            )],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Blocked(BlockedReason::JournalpostTilstandUavklart)
        ));
    }

    #[test]
    fn dokument_feilet_permanent_gir_invalid() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Opprettet,
            JournalpostType::Utgaaende,
            false,
            vec![dok(DokumentTilstand::FeiletPermanent)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::DokumentFeiletPermanent)
        ));
    }

    #[test]
    fn avslutt_sak_journalpost_feilet_permanent_gir_invalid() {
        // Journalpost in FeiletPermanent state with no failed documents
        // should return Invalid(JournalpostFeiletPermanent), not Blocked(JournalposterIkkeFerdige)
        let jp = lag_journalpost(
            jp_id(),
            JournalpostTilstand::FeiletPermanent,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::JournalpostFeiletPermanent)
        ));
    }

    #[test]
    fn avslutt_sak_dokument_feilet_permanent_gir_invalid() {
        let jp = lag_journalpost(
            jp_id(),
            JournalpostTilstand::Avskrevet,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::FeiletPermanent)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision =
            planlegg_neste_handling(CommandTypeCode::AvsluttSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Invalid(DomainViolation::DokumentFeiletPermanent)
        ));
    }

    #[test]
    fn avskriv_baseres_paa_journalposttype_ikke_lagret_sluttilstand() {
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Inngaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak = opprettet_sak_med_saksnummer(vec![jp]);

        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );

        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::Avskriv { .. })
        ));
    }

    #[test]
    fn utgaaende_og_internt_notat_ved_journalfoert_gir_done_ikke_avskriv() {
        let jp_id_1 = jp_id();
        let jp_ut = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::Journalfoert,
            JournalpostType::Utgaaende,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak_ut = opprettet_sak_med_saksnummer(vec![jp_ut]);

        let decision_ut = planlegg_neste_handling(
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak_ut,
        );
        assert!(matches!(decision_ut, CommandStateDecision::Done));

        let jp_id_2 = jp_id();
        let jp_internt = lag_journalpost(
            jp_id_2,
            JournalpostTilstand::Journalfoert,
            JournalpostType::InterntNotat,
            false,
            vec![dok(DokumentTilstand::Ok)],
        );
        let sak_internt = opprettet_sak_med_saksnummer(vec![jp_internt]);

        let decision_internt = planlegg_neste_handling(
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandTarget::Journalpost(jp_id_2),
            &sak_internt,
        );
        assert!(matches!(decision_internt, CommandStateDecision::Done));
    }

    // =========================================================================
    // Full lifecycle tests
    // =========================================================================

    #[test]
    fn full_lifecycle_inngaende_journalpost() {
        let d = dok(DokumentTilstand::IkkeRealisert);
        let jp_id_1 = jp_id();
        let jp = lag_journalpost(
            jp_id_1,
            JournalpostTilstand::IkkeRealisert,
            JournalpostType::Inngaende,
            false,
            vec![d],
        );

        // Step 1: Create sak (use OpprettSak command type)
        let mut sak = SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::IkkeRealisert,
            sikri_id: None,
            saksnummer: None,
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![jp],
        };
        let decision =
            planlegg_neste_handling(CommandTypeCode::OpprettSak, CommandTarget::Sak, &sak);
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::OpprettSak { .. })
        ));

        // Step 2: Create journalpost
        sak.tilstand = SakTilstand::Opprettet;
        sak.sikri_id = Some(1);
        sak.saksnummer = Some("2025/1".to_string());
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::OpprettJournalpost { .. })
        ));

        // Step 3: Add document
        sak.journalposter[0].tilstand = JournalpostTilstand::Opprettet;
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::LeggTilDokument { .. })
        ));

        // Step 4: Journalføre
        sak.journalposter[0].dokumenter[0].tilstand = DokumentTilstand::Ok;
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::Journalfoer { .. })
        ));

        // Step 5: Avskriv
        sak.journalposter[0].tilstand = JournalpostTilstand::Journalfoert;
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(
            decision,
            CommandStateDecision::Ready(ArkivOperasjon::Avskriv { .. })
        ));

        // Step 6: Done
        sak.journalposter[0].tilstand = JournalpostTilstand::Avskrevet;
        let decision = planlegg_neste_handling(
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandTarget::Journalpost(jp_id_1),
            &sak,
        );
        assert!(matches!(decision, CommandStateDecision::Done));
    }

    // =========================================================================
    // Compatibility wrapper tests
    // =========================================================================

    #[allow(deprecated)]
    mod compatibility_wrapper_tests {
        use super::*;

        #[test]
        fn neste_handling_compat_ready_gir_ok_some() {
            let sak = enkel_sak(SakTilstand::IkkeRealisert);
            let result =
                neste_handling(CommandTypeCode::OpprettSak, CommandTarget::Sak, &sak).unwrap();
            assert!(result.is_some());
        }

        #[test]
        fn neste_handling_compat_done_gir_ok_none() {
            let mut sak = enkel_sak(SakTilstand::Opprettet);
            sak.saksnummer = Some("2025/1".to_string());
            let result =
                neste_handling(CommandTypeCode::OpprettSak, CommandTarget::Sak, &sak).unwrap();
            assert!(result.is_none());
        }

        #[test]
        fn neste_handling_compat_invalid_gir_err() {
            let jp_id_1 = jp_id();
            let jp = lag_journalpost(
                jp_id_1,
                JournalpostTilstand::Opprettet,
                JournalpostType::Utgaaende,
                false,
                vec![dok(DokumentTilstand::FeiletPermanent)],
            );
            let sak = opprettet_sak_med_saksnummer(vec![jp]);

            let result = neste_handling(
                CommandTypeCode::OpprettUtgaaendeJournalpost,
                CommandTarget::Journalpost(jp_id_1),
                &sak,
            );
            assert!(result.is_err());
        }
    }
}
