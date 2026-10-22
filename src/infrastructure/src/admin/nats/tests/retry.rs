use super::*;
use crate::admin::nats::svar::{INTERNAL_ERROR, INVALID_REQUEST};

#[tokio::test]
async fn teknisk_feil_gir_sanitert_feilsvar_paa_begge_retry_subjects() {
    let transport = Arc::new(FakeTransport::default());
    let fakes = Fakes {
        retry: Arc::new(FakeRetryRepository {
            teknisk_feil: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let id = Uuid::new_v4();
    transport.meldinger.lock().unwrap().extend([
        (
            AdminRute::RetrySak.subject(),
            vec![melding(
                "reply.sak",
                json!({"utfort_av": "op", "key": {"type": "skuffenId", "value": id}}),
            )],
        ),
        (
            AdminRute::RetryCommands.subject(),
            vec![melding(
                "reply.commands",
                json!({"utfort_av": "op", "command_ids": [id]}),
            )],
        ),
    ]);
    let listener = fakes.listener(transport.clone());

    kjor_til_svar(&listener, &transport, 2).await;

    for svar in transport.publiserte_svar() {
        assert_eq!(
            svar,
            json!({"status": "Error", "payload": {"message": INTERNAL_ERROR}})
        );
    }
}

#[tokio::test]
async fn feil_shape_paa_begge_retry_subjects_avvises_uten_servicekall() {
    let transport = Arc::new(FakeTransport::default());
    let fakes = Fakes::default();
    let id = Uuid::new_v4();
    let key = json!({"type": "skuffenId", "value": id});
    let ugyldige = [
        json!({"utfort_av": "op"}),
        json!({"utfort_av": "op", "key": key, "command_ids": [id]}),
        json!({"utfort_av": "op", "key": null}),
        json!({"utfort_av": "op", "command_ids": null}),
        json!({"utfort_av": "op", "key": key, "command_ids": null}),
        json!({"utfort_av": "op", "key": null, "command_ids": [id]}),
        json!({"utfort_av": "op", "key": key, "extra": true}),
        json!({"utfort_av": "op", "command_ids": [id], "extra": true}),
        json!({"utfort_av": "op", "key": {"type": "ukjent", "value": id}}),
        json!({"utfort_av": "op", "key": {"type": "skuffenId", "value": id, "extra": true}}),
        json!({"key": key}),
        json!({"command_ids": [id]}),
        json!(null),
    ];
    let mut antall = 0;
    for rute in [AdminRute::RetrySak, AdminRute::RetryCommands] {
        let (feil_shape, maal) = match rute {
            AdminRute::RetrySak => (json!({"utfort_av": "op", "command_ids": [id]}), "key"),
            _ => (json!({"utfort_av": "op", "key": key}), "command_ids"),
        };
        let mut meldinger: Vec<_> = ugyldige
            .iter()
            .cloned()
            .chain([feil_shape])
            .map(|wire| melding("reply.invalid", wire))
            .collect();
        for utfort_av in [" ".to_string(), "a\nb".to_string(), "x".repeat(129)] {
            let mut wire = json!({"utfort_av": utfort_av});
            wire[maal] = if maal == "key" {
                key.clone()
            } else {
                json!([id])
            };
            meldinger.push(melding("reply.invalid", wire));
        }
        meldinger.push(AdminMessage {
            reply: Some("reply.invalid".to_string()),
            headers: None,
            payload: "{ugyldig json".into(),
        });
        antall += meldinger.len();
        transport
            .meldinger
            .lock()
            .unwrap()
            .insert(rute.subject(), meldinger);
    }
    let listener = fakes.listener(transport.clone());

    kjor_til_svar(&listener, &transport, antall).await;

    assert!(fakes.retry.sak_kall.lock().unwrap().is_empty());
    assert!(fakes.retry.command_kall.lock().unwrap().is_empty());
    for (reply, svar) in transport.publisert.lock().unwrap().iter() {
        assert_eq!(reply, "reply.invalid");
        assert_eq!(
            *svar,
            json!({"status": "Error", "payload": {"message": INVALID_REQUEST}})
        );
    }
}
