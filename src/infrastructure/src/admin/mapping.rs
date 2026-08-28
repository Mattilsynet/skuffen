//! All oversettelse mellom `lib_schemas::skuffen::admin` og application-modellen.
//!
//! `domain` og `application` har ingen `lib-schemas`-avhengighet; mappingen bor
//! her (SKU-0013).

use application::admin::model::{
    AdminCommand, AdminCommandUtfall, AdminDokument, AdminEntitetIdentitet, AdminJournalpost,
    AdminKorrespondansepart, AdminOperasjonDetaljer, AdminOperasjonEntitet,
    AdminOperasjonSammendrag, AdminSak, AdminSakFakta, AdminSakNokkel,
};
use domain::eksekvering::operasjon::EntitetId;
use lib_schemas::skuffen::admin::{
    AdminCommandResponseV1, AdminCommandUtfallV1, AdminDokumentV1, AdminEntitetIdentitetV1,
    AdminJournalpostV1, AdminKorrespondansepartV1, AdminOperasjonDetaljerV1,
    AdminOperasjonEntitetV1, AdminOperasjonSammendragV1, AdminSakFaktaV1, AdminSakKeyV1,
    AdminSakResponseV1,
};

pub fn til_sak_nokkel(key: AdminSakKeyV1) -> AdminSakNokkel {
    match key {
        AdminSakKeyV1::SkuffenId(skuffen_id) => AdminSakNokkel::SkuffenId(skuffen_id.into()),
        AdminSakKeyV1::ClientReference(client_reference) => {
            AdminSakNokkel::ClientReference(client_reference)
        }
        AdminSakKeyV1::ArkivId(arkiv_id) => AdminSakNokkel::ArkivId(arkiv_id),
    }
}

pub fn til_command_response(command: AdminCommand) -> AdminCommandResponseV1 {
    let utfall = til_utfall(command.utled_utfall());
    AdminCommandResponseV1 {
        command_id: command.command_id,
        correlation_id: command.correlation_id,
        command_type: command.command_type,
        mottatt_at: command.mottatt_at,
        dispatchet_at: command.dispatchet_at,
        dekomponert_at: command.dekomponert_at,
        utfall,
        operasjoner: command
            .operasjoner
            .into_iter()
            .map(til_operasjon_detaljer)
            .collect(),
    }
}

pub fn til_sak_response(sak: AdminSak) -> AdminSakResponseV1 {
    AdminSakResponseV1 {
        identitet: til_entitet_identitet(sak.identitet),
        fakta: sak.fakta.map(til_sak_fakta),
        operasjoner: sak
            .operasjoner
            .into_iter()
            .map(til_operasjon_sammendrag)
            .collect(),
    }
}

fn til_utfall(utfall: AdminCommandUtfall) -> AdminCommandUtfallV1 {
    match utfall {
        AdminCommandUtfall::Uavklart => AdminCommandUtfallV1::Uavklart,
        AdminCommandUtfall::KreverAvklaring => AdminCommandUtfallV1::KreverAvklaring,
        AdminCommandUtfall::Fullfort => AdminCommandUtfallV1::Fullfort,
        AdminCommandUtfall::Feilet => AdminCommandUtfallV1::Feilet,
    }
}

fn entitet_type(skuffen_id: EntitetId) -> String {
    skuffen_id.entitet_type().as_code().to_string()
}

fn til_entitet_identitet(identitet: AdminEntitetIdentitet) -> AdminEntitetIdentitetV1 {
    AdminEntitetIdentitetV1 {
        skuffen_id: identitet.skuffen_id.as_uuid(),
        entitet_type: entitet_type(identitet.skuffen_id),
        client_reference: identitet.client_reference,
        arkiv_id: identitet.arkiv_id,
        created_at: identitet.created_at,
        updated_at: identitet.updated_at,
    }
}

fn til_operasjon_entitet(entitet: AdminOperasjonEntitet) -> AdminOperasjonEntitetV1 {
    AdminOperasjonEntitetV1 {
        skuffen_id: entitet.skuffen_id.as_uuid(),
        entitet_type: entitet_type(entitet.skuffen_id),
        client_reference: entitet.client_reference,
        arkiv_id: entitet.arkiv_id,
    }
}

fn til_operasjon_detaljer(operasjon: AdminOperasjonDetaljer) -> AdminOperasjonDetaljerV1 {
    AdminOperasjonDetaljerV1 {
        operasjon_id: operasjon.operasjon_id.into(),
        operasjonstype: operasjon.operasjonstype,
        entitet: til_operasjon_entitet(operasjon.entitet),
        sak_id: operasjon.sak_id.into(),
        status: operasjon.status,
        attempt_no: operasjon.attempt_no,
        neste_forsok_at: operasjon.neste_forsok_at,
        blokkert_av: operasjon.blokkert_av,
        siste_detalj: operasjon.siste_detalj,
        sendt_at: operasjon.sendt_at,
        ferdig_at: operasjon.ferdig_at,
        varslet_at: operasjon.varslet_at,
        created_at: operasjon.created_at,
        updated_at: operasjon.updated_at,
    }
}

fn til_operasjon_sammendrag(operasjon: AdminOperasjonSammendrag) -> AdminOperasjonSammendragV1 {
    AdminOperasjonSammendragV1 {
        operasjon_id: operasjon.operasjon_id.into(),
        command_id: operasjon.command_id,
        operasjonstype: operasjon.operasjonstype,
        entitet_id: operasjon.entitet_id.as_uuid(),
        status: operasjon.status,
    }
}

fn til_sak_fakta(fakta: AdminSakFakta) -> AdminSakFaktaV1 {
    AdminSakFaktaV1 {
        tilstand: fakta.tilstand,
        sakstittel: fakta.sakstittel,
        arkivdel: fakta.arkivdel,
        ordningsverdi: fakta.ordningsverdi,
        opprettelse_saksbehandler_id: fakta.opprettelse_saksbehandler_id,
        opprettelse_saksbehandler_enhet: fakta.opprettelse_saksbehandler_enhet,
        tilgangskode: fakta.tilgangskode,
        tilgangshjemmel: fakta.tilgangshjemmel,
        oensket_saksansvarlig_id: fakta.oensket_saksansvarlig_id,
        oensket_saksansvarlig_enhet: fakta.oensket_saksansvarlig_enhet,
        naavaerende_saksansvarlig_id: fakta.naavaerende_saksansvarlig_id,
        naavaerende_saksansvarlig_enhet: fakta.naavaerende_saksansvarlig_enhet,
        opprettet_av_command_id: fakta.opprettet_av_command_id,
        created_at: fakta.created_at,
        updated_at: fakta.updated_at,
        journalposter: fakta
            .journalposter
            .into_iter()
            .map(til_journalpost)
            .collect(),
    }
}

fn til_journalpost(journalpost: AdminJournalpost) -> AdminJournalpostV1 {
    AdminJournalpostV1 {
        identitet: til_entitet_identitet(journalpost.identitet),
        sak_id: journalpost.sak_id.into(),
        tilstand: journalpost.tilstand,
        journalposttype: journalpost.journalposttype,
        med_utsending: journalpost.med_utsending,
        tittel: journalpost.tittel,
        dokument_dato: journalpost.dokument_dato,
        saksbehandler_id: journalpost.saksbehandler_id,
        saksbehandler_enhet: journalpost.saksbehandler_enhet,
        tilgangskode: journalpost.tilgangskode,
        tilgangshjemmel: journalpost.tilgangshjemmel,
        korrespondanseparter: journalpost
            .korrespondanseparter
            .map(|parter| parter.into_iter().map(til_korrespondansepart).collect()),
        kildesystem: journalpost.kildesystem,
        opprettet_av_command_id: journalpost.opprettet_av_command_id,
        created_at: journalpost.created_at,
        updated_at: journalpost.updated_at,
        dokumenter: journalpost
            .dokumenter
            .into_iter()
            .map(til_dokument)
            .collect(),
    }
}

fn til_korrespondansepart(part: AdminKorrespondansepart) -> AdminKorrespondansepartV1 {
    AdminKorrespondansepartV1 {
        rolle: part.rolle,
        navn: part.navn,
        parttype: part.parttype,
        id_type: part.id_type,
        id: part.id,
        adresse: part.adresse,
        postnummer: part.postnummer,
        poststed: part.poststed,
    }
}

fn til_dokument(dokument: AdminDokument) -> AdminDokumentV1 {
    AdminDokumentV1 {
        identitet: til_entitet_identitet(dokument.identitet),
        journalpost_id: dokument.journalpost_id,
        tilstand: dokument.tilstand,
        rekkefolge: dokument.rekkefolge,
        er_hoveddokument: dokument.er_hoveddokument,
        tittel: dokument.tittel,
        filtype: dokument.filtype,
        dokument_referanse: dokument.dokument_referanse,
        mal_referanse: dokument.mal_referanse,
        felter: dokument.felter,
        rendered_dokument_referanse: dokument.rendered_dokument_referanse,
        opprettet_av_command_id: dokument.opprettet_av_command_id,
        created_at: dokument.created_at,
        updated_at: dokument.updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
    use domain::eksekvering::operasjon::OperasjonId;
    use uuid::Uuid;

    fn tidspunkt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-27T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn identitet(skuffen_id: EntitetId) -> AdminEntitetIdentitet {
        AdminEntitetIdentitet {
            skuffen_id,
            client_reference: None,
            arkiv_id: None,
            created_at: tidspunkt(),
            updated_at: tidspunkt(),
        }
    }

    #[test]
    fn wire_key_mappes_til_alle_tre_interne_varianter() {
        let id = Uuid::new_v4();

        assert_eq!(
            til_sak_nokkel(AdminSakKeyV1::SkuffenId(id)),
            AdminSakNokkel::SkuffenId(SkuffenSakId(id))
        );
        assert_eq!(
            til_sak_nokkel(AdminSakKeyV1::ClientReference(id)),
            AdminSakNokkel::ClientReference(id)
        );
        assert_eq!(
            til_sak_nokkel(AdminSakKeyV1::ArkivId("2026/12345".to_string())),
            AdminSakNokkel::ArkivId("2026/12345".to_string())
        );
    }

    #[test]
    fn entitet_type_utledes_fra_intern_id() {
        let id = Uuid::new_v4();
        assert_eq!(entitet_type(EntitetId::Sak(SkuffenSakId(id))), "sak");
        assert_eq!(
            entitet_type(EntitetId::Journalpost(SkuffenJournalpostId(id))),
            "journalpost"
        );
        assert_eq!(
            entitet_type(EntitetId::Dokument(SkuffenDokumentId(id))),
            "dokument"
        );
    }

    #[test]
    fn alle_fire_utfall_mappes() {
        assert_eq!(
            til_utfall(AdminCommandUtfall::Uavklart),
            AdminCommandUtfallV1::Uavklart
        );
        assert_eq!(
            til_utfall(AdminCommandUtfall::KreverAvklaring),
            AdminCommandUtfallV1::KreverAvklaring
        );
        assert_eq!(
            til_utfall(AdminCommandUtfall::Fullfort),
            AdminCommandUtfallV1::Fullfort
        );
        assert_eq!(
            til_utfall(AdminCommandUtfall::Feilet),
            AdminCommandUtfallV1::Feilet
        );
    }

    #[test]
    fn command_mapping_folder_utfall_og_beholder_lagrede_koder() {
        let command_id = Uuid::new_v4();
        let sak_id = SkuffenSakId(Uuid::new_v4());
        let command = AdminCommand {
            command_id,
            correlation_id: None,
            command_type: "ukjent_historisk_type".to_string(),
            mottatt_at: tidspunkt(),
            dispatchet_at: None,
            dekomponert_at: None,
            operasjoner: vec![AdminOperasjonDetaljer {
                operasjon_id: OperasjonId(Uuid::new_v4()),
                operasjonstype: "opprett_sak".to_string(),
                entitet: AdminOperasjonEntitet {
                    skuffen_id: EntitetId::Sak(sak_id),
                    client_reference: None,
                    arkiv_id: Some("".to_string()),
                },
                sak_id,
                status: "krever_avklaring".to_string(),
                attempt_no: 2,
                neste_forsok_at: None,
                blokkert_av: None,
                siste_detalj: None,
                sendt_at: None,
                ferdig_at: None,
                varslet_at: None,
                created_at: tidspunkt(),
                updated_at: tidspunkt(),
            }],
        };

        let response = til_command_response(command);

        assert_eq!(response.utfall, AdminCommandUtfallV1::KreverAvklaring);
        assert_eq!(response.command_type, "ukjent_historisk_type");
        assert_eq!(response.operasjoner[0].entitet.entitet_type, "sak");
        assert_eq!(
            response.operasjoner[0].entitet.arkiv_id.as_deref(),
            Some("")
        );
    }

    #[test]
    fn identity_only_sak_mappes_med_fakta_none() {
        let sak_id = SkuffenSakId(Uuid::new_v4());
        let sak = AdminSak {
            identitet: identitet(EntitetId::Sak(sak_id)),
            fakta: None,
            operasjoner: Vec::new(),
        };

        let response = til_sak_response(sak);

        assert!(response.fakta.is_none());
        assert_eq!(response.identitet.entitet_type, "sak");
        assert_eq!(response.identitet.skuffen_id, sak_id.0);
    }

    #[test]
    fn optional_storage_felt_mappes_uten_command_side_validering() {
        let sak_id = SkuffenSakId(Uuid::new_v4());
        let journalpost_id = SkuffenJournalpostId(Uuid::new_v4());
        let dokument_id = SkuffenDokumentId(Uuid::new_v4());
        let command_id = Uuid::new_v4();

        let sak = AdminSak {
            identitet: identitet(EntitetId::Sak(sak_id)),
            fakta: Some(AdminSakFakta {
                tilstand: "opprettet".to_string(),
                sakstittel: None,
                arkivdel: None,
                ordningsverdi: None,
                opprettelse_saksbehandler_id: Some("A".to_string()),
                opprettelse_saksbehandler_enhet: None,
                tilgangskode: Some("".to_string()),
                tilgangshjemmel: Some("utgått hjemmel".to_string()),
                oensket_saksansvarlig_id: Some("B".to_string()),
                oensket_saksansvarlig_enhet: None,
                naavaerende_saksansvarlig_id: Some("C".to_string()),
                naavaerende_saksansvarlig_enhet: None,
                opprettet_av_command_id: command_id,
                created_at: tidspunkt(),
                updated_at: tidspunkt(),
                journalposter: vec![AdminJournalpost {
                    identitet: identitet(EntitetId::Journalpost(journalpost_id)),
                    sak_id,
                    tilstand: "opprettet".to_string(),
                    journalposttype: "X".to_string(),
                    med_utsending: false,
                    tittel: None,
                    dokument_dato: None,
                    saksbehandler_id: Some("D".to_string()),
                    saksbehandler_enhet: None,
                    tilgangskode: None,
                    tilgangshjemmel: None,
                    korrespondanseparter: Some(Vec::new()),
                    kildesystem: None,
                    opprettet_av_command_id: command_id,
                    created_at: tidspunkt(),
                    updated_at: tidspunkt(),
                    dokumenter: vec![AdminDokument {
                        identitet: identitet(EntitetId::Dokument(dokument_id)),
                        journalpost_id: journalpost_id.0,
                        tilstand: "klar".to_string(),
                        rekkefolge: 0,
                        er_hoveddokument: true,
                        tittel: None,
                        filtype: None,
                        dokument_referanse: None,
                        mal_referanse: None,
                        felter: None,
                        rendered_dokument_referanse: None,
                        opprettet_av_command_id: command_id,
                        created_at: tidspunkt(),
                        updated_at: tidspunkt(),
                    }],
                }],
            }),
            operasjoner: vec![AdminOperasjonSammendrag {
                operasjon_id: OperasjonId(Uuid::new_v4()),
                command_id,
                operasjonstype: "opprett_journalpost".to_string(),
                entitet_id: EntitetId::Journalpost(journalpost_id),
                status: "blokkert".to_string(),
            }],
        };

        let response = til_sak_response(sak);
        let fakta = response.fakta.expect("fakta finnes");

        assert_eq!(fakta.tilgangskode.as_deref(), Some(""));
        assert_eq!(fakta.opprettelse_saksbehandler_id.as_deref(), Some("A"));
        assert_eq!(fakta.oensket_saksansvarlig_id.as_deref(), Some("B"));
        assert_eq!(fakta.naavaerende_saksansvarlig_id.as_deref(), Some("C"));
        assert_eq!(
            fakta.journalposter[0].saksbehandler_id.as_deref(),
            Some("D")
        );
        // SQL NULL og tom liste holdes fra hverandre hele veien ut.
        assert_eq!(
            fakta.journalposter[0].korrespondanseparter,
            Some(Vec::new())
        );
        assert_eq!(fakta.journalposter[0].dokumenter[0].felter, None);
        assert_eq!(
            fakta.journalposter[0].dokumenter[0].identitet.entitet_type,
            "dokument"
        );
        assert_eq!(response.operasjoner[0].entitet_id, journalpost_id.0);
    }
}
