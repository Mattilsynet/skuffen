//! Admin retry mot ekte PostgreSQL: frigivelse, sperren mot uavklart utfall,
//! låsing mot pågående arbeid og idempotent dekomponering ved replay.

use application::admin::model::AdminSakNokkel;
use application::admin::ports::admin_retry_port::{AdminRetryRepository, FrigivelseFeil};
use application::command::materialisering::{Dekomponeringsplan, OperasjonRad, SakRad};
use application::command::ports::operasjon_port::OperasjonRepository;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::{EntitetId, OperasjonId, Operasjonsstatus, Operasjonstype};
use domain::eksekvering::typer::CommandTypeCode;
use infrastructure::admin::adapter::postgres_admin_retry_repository::PostgresAdminRetryRepository;
use infrastructure::command::adapter::postgres_operasjon_repository::PostgresOperasjonRepository;
use lib_sql::database_config::DbPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

const SAK: Uuid = Uuid::from_u128(0x5a);
const ANNEN_SAK: Uuid = Uuid::from_u128(0x5b);
const RETRY: Uuid = Uuid::from_u128(0xc1);
const ANNEN_COMMAND: Uuid = Uuid::from_u128(0xc2);
const AVVIST: Uuid = Uuid::from_u128(0xc3);

struct Fixture {
    _container: test_docker::Container,
    pool: DbPool,
}

fn connect_options(port: u16) -> PgConnectOptions {
    PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("postgres")
        .password("postgres")
        .database("postgres")
}

async fn start() -> Fixture {
    let (container, port) = test_docker::postgres().expect("postgres startet");
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(connect_options(port))
        .await
        .expect("koblet til postgres");
    infrastructure::database::setup::run_migrations(&pool)
        .await
        .expect("migrasjoner kjørte");
    Fixture {
        _container: container,
        pool,
    }
}

async fn sak(pool: &DbPool, sak_id: Uuid, command_id: Uuid) {
    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference) VALUES ($1, 'sak', $2)",
    )
    .bind(sak_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .unwrap();
    command(pool, command_id, "opprett_sak", Some(sak_id), true).await;
    sqlx::query(
        "INSERT INTO sak_tilstand (sak_id, tilstand, opprettet_av_command_id)
         VALUES ($1, 'opprettet', $2)",
    )
    .bind(sak_id)
    .bind(command_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn command(
    pool: &DbPool,
    command_id: Uuid,
    command_type: &str,
    sak_id: Option<Uuid>,
    dekomponert: bool,
) {
    sqlx::query(
        "INSERT INTO command (command_id, command_type, sak_id, dispatchet_at, dekomponert_at)
         VALUES ($1, $2, $3, now(), CASE WHEN $4 THEN now() END)",
    )
    .bind(command_id)
    .bind(command_type)
    .bind(sak_id)
    .bind(dekomponert)
    .execute(pool)
    .await
    .unwrap();
}

/// Operasjon i gitt status, med ett avsluttet forsøk som historikk. Hver
/// operasjon får sin egen entitet, så flere kan tilhøre samme kommando.
async fn operasjon(pool: &DbPool, command_id: Uuid, sak_id: Uuid, status: &str) -> Uuid {
    let operasjon_id = Uuid::now_v7();
    let entitet_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference)
         VALUES ($1, 'journalpost', $2)",
    )
    .bind(entitet_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO operasjon
             (operasjon_id, command_id, operasjonstype, entitet_id, sak_id, status,
              attempt_no, neste_forsok_at, ferdig_at, siste_detalj)
         VALUES ($1, $2, 'journalfor', $3, $4, $5::operasjon_status, 1,
                 now() + interval '1 hour',
                 CASE WHEN $5 IN ('ok', 'feilet') THEN now() END,
                 'sikri_unknown_user')",
    )
    .bind(operasjon_id)
    .bind(command_id)
    .bind(entitet_id)
    .bind(sak_id)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO operasjon_forsok (operasjon_id, attempt_no, executor_id, avsluttet_at, utfall)
         VALUES ($1, 1, 'test', now(), 'feilet')",
    )
    .bind(operasjon_id)
    .execute(pool)
    .await
    .unwrap();
    operasjon_id
}

#[derive(Debug, PartialEq, Eq)]
struct Rad {
    status: String,
    attempt_no: i32,
    forfalt: bool,
    ferdig: bool,
    siste_detalj: Option<String>,
    forsok: i64,
}

async fn rad(pool: &DbPool, operasjon_id: Uuid) -> Rad {
    let (status, attempt_no, forfalt, ferdig, siste_detalj, forsok): (
        String,
        i32,
        bool,
        bool,
        Option<String>,
        i64,
    ) = sqlx::query_as(
        "SELECT status::text, attempt_no, neste_forsok_at <= now(), ferdig_at IS NOT NULL,
                siste_detalj,
                (SELECT count(*) FROM operasjon_forsok f WHERE f.operasjon_id = o.operasjon_id)
         FROM operasjon o WHERE operasjon_id = $1",
    )
    .bind(operasjon_id)
    .fetch_one(pool)
    .await
    .unwrap();
    Rad {
        status,
        attempt_no,
        forfalt,
        ferdig,
        siste_detalj,
        forsok,
    }
}

fn repo(pool: &DbPool) -> PostgresAdminRetryRepository {
    PostgresAdminRetryRepository::new(pool.clone())
}

fn sak_nokler(skuffen_id: Uuid, client_reference: Uuid, arkiv_id: &str) -> [AdminSakNokkel; 3] {
    [
        AdminSakNokkel::SkuffenId(SkuffenSakId(skuffen_id)),
        AdminSakNokkel::ClientReference(client_reference),
        AdminSakNokkel::ArkivId(arkiv_id.to_string()),
    ]
}

#[tokio::test]
async fn alle_saksnokler_finner_identity_only_sak_med_og_uten_commands() {
    let f = start().await;
    let client_reference = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference, arkiv_id)
         VALUES ($1, 'sak', $2, '2026/1')",
    )
    .bind(SAK)
    .bind(client_reference)
    .execute(&f.pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, arkiv_id)
         VALUES ($1, 'journalpost', '2026/1')",
    )
    .bind(Uuid::new_v4())
    .execute(&f.pool)
    .await
    .unwrap();

    for key in sak_nokler(SAK, client_reference, "2026/1") {
        assert_eq!(
            repo(&f.pool).hent_commands_for_sak(key, 101).await.unwrap(),
            Some(vec![]),
            "kjent identitet uten sak_tilstand er en tom sak"
        );
    }

    command(&f.pool, AVVIST, "avslutt_sak", Some(SAK), false).await;
    for key in sak_nokler(SAK, client_reference, "2026/1") {
        assert_eq!(
            repo(&f.pool).hent_commands_for_sak(key, 101).await.unwrap(),
            Some(vec![AVVIST]),
            "koblet command uten dekomponering eller operasjoner tas med"
        );
    }
}

#[tokio::test]
async fn ukjente_nokler_og_feil_entitetstype_gir_ingen_sak() {
    let f = start().await;
    for key in sak_nokler(SAK, Uuid::new_v4(), "ukjent-sak") {
        assert_eq!(
            repo(&f.pool).hent_commands_for_sak(key, 101).await.unwrap(),
            None
        );
    }

    for entitet_type in ["journalpost", "dokument"] {
        let skuffen_id = Uuid::new_v4();
        let client_reference = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO entitet (skuffen_id, entitet_type, client_reference, arkiv_id)
             VALUES ($1, $2::entitet_type, $3, '2026/1')",
        )
        .bind(skuffen_id)
        .bind(entitet_type)
        .bind(client_reference)
        .execute(&f.pool)
        .await
        .unwrap();
        command(
            &f.pool,
            Uuid::new_v4(),
            "avslutt_sak",
            Some(skuffen_id),
            false,
        )
        .await;

        for key in sak_nokler(skuffen_id, client_reference, "2026/1") {
            assert_eq!(
                repo(&f.pool).hent_commands_for_sak(key, 101).await.unwrap(),
                None,
                "{entitet_type} er ikke en sak, selv med en koblet command"
            );
        }
    }
}

#[tokio::test]
async fn saksutvalg_bruker_kobling_eller_historisk_fallback_uten_duplikater() {
    let f = start().await;
    sak(&f.pool, SAK, AVVIST).await;
    sak(&f.pool, ANNEN_SAK, ANNEN_COMMAND).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", None, true).await;
    operasjon(&f.pool, RETRY, SAK, "feilet").await;
    operasjon(&f.pool, RETRY, SAK, "ok").await;

    let koblet = Uuid::from_u128(0xc4);
    command(&f.pool, koblet, "avslutt_sak", Some(SAK), true).await;
    operasjon(&f.pool, koblet, SAK, "retry_venter").await;
    operasjon(&f.pool, koblet, SAK, "blokkert").await;
    command(&f.pool, Uuid::new_v4(), "avslutt_sak", None, false).await;
    let historisk_annen = Uuid::new_v4();
    command(&f.pool, historisk_annen, "avslutt_sak", None, true).await;
    operasjon(&f.pool, historisk_annen, ANNEN_SAK, "feilet").await;
    operasjon(&f.pool, ANNEN_COMMAND, SAK, "feilet").await;

    sqlx::query(
        "UPDATE command SET mottatt_at = CASE WHEN command_id = $1
         THEN timestamptz '2026-01-01 00:00:00+00'
         ELSE timestamptz '2026-01-02 00:00:00+00' END",
    )
    .bind(koblet)
    .execute(&f.pool)
    .await
    .unwrap();

    for (grense, forventet) in [
        (101, vec![koblet, RETRY, AVVIST]),
        (2, vec![koblet, RETRY]),
        (0, vec![]),
    ] {
        assert_eq!(
            repo(&f.pool)
                .hent_commands_for_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(SAK)), grense)
                .await
                .unwrap(),
            Some(forventet),
            "sortert på mottatt_at og command_id; eksplisitt kobling vinner over historikk"
        );
    }
}

#[tokio::test]
async fn saksutvalg_returnerer_inntil_101_commands_for_servicegrensen() {
    let f = start().await;
    sak(&f.pool, SAK, AVVIST).await;
    sqlx::query("UPDATE command SET mottatt_at = timestamptz '2026-01-02 00:00:00+00'")
        .execute(&f.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO command (command_id, command_type, sak_id, mottatt_at)
         SELECT md5(n::text)::uuid, 'avslutt_sak', $1, timestamptz '2026-01-01 00:00:00+00'
         FROM generate_series(1, 102) n",
    )
    .bind(SAK)
    .execute(&f.pool)
    .await
    .unwrap();
    let alle: Vec<Uuid> =
        sqlx::query_scalar("SELECT command_id FROM command WHERE command_id <> $1")
            .bind(AVVIST)
            .fetch_all(&f.pool)
            .await
            .unwrap();
    let mut forventet = alle;
    forventet.sort();
    forventet.truncate(101);

    assert_eq!(
        repo(&f.pool)
            .hent_commands_for_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(SAK)), 101)
            .await
            .unwrap(),
        Some(forventet)
    );
}

#[tokio::test]
async fn grunnlaget_viser_milepael_sak_og_operasjonsstatuser() {
    let f = start().await;
    sak(&f.pool, SAK, Uuid::new_v4()).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", None, true).await;
    operasjon(&f.pool, RETRY, SAK, "feilet").await;
    command(&f.pool, AVVIST, "avslutt_sak", Some(SAK), false).await;

    let mut grunnlag = repo(&f.pool)
        .hent_grunnlag(&[RETRY, AVVIST, Uuid::new_v4()])
        .await
        .unwrap();
    grunnlag.sort_by_key(|g| g.command_id);

    assert_eq!(grunnlag.len(), 2, "ukjente kommandoer utelates");
    assert_eq!(grunnlag[0].command_id, RETRY);
    assert_eq!(grunnlag[0].command_type, CommandTypeCode::SettSaksansvarlig);
    assert_eq!(
        grunnlag[0].sak_id,
        Some(SkuffenSakId(SAK)),
        "manglende kobling faller tilbake på operasjonene"
    );
    assert!(grunnlag[0].dekomponert);
    assert_eq!(grunnlag[0].operasjoner, vec![Operasjonsstatus::Feilet]);

    assert_eq!(grunnlag[1].command_id, AVVIST);
    assert!(!grunnlag[1].dekomponert);
    assert!(grunnlag[1].operasjoner.is_empty());
}

#[tokio::test]
async fn frigivelse_gjenaapner_feilet_og_bevarer_historikken() {
    let f = start().await;
    sak(&f.pool, SAK, Uuid::new_v4()).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", Some(SAK), true).await;
    let feilet = operasjon(&f.pool, RETRY, SAK, "feilet").await;
    let venter = operasjon(&f.pool, RETRY, SAK, "retry_venter").await;
    let blokkert = operasjon(&f.pool, RETRY, SAK, "blokkert").await;
    let ok = operasjon(&f.pool, RETRY, SAK, "ok").await;
    let kjorer = operasjon(&f.pool, RETRY, SAK, "kjorer").await;
    let sendt = operasjon(&f.pool, RETRY, SAK, "sendt").await;
    command(
        &f.pool,
        ANNEN_COMMAND,
        "sett_saksansvarlig",
        Some(SAK),
        true,
    )
    .await;
    let annen = operasjon(&f.pool, ANNEN_COMMAND, SAK, "feilet").await;

    repo(&f.pool)
        .frigi(&[SkuffenSakId(SAK)], &[RETRY])
        .await
        .unwrap();

    let gjenaapnet = rad(&f.pool, feilet).await;
    assert_eq!(gjenaapnet.status, "klar");
    assert!(gjenaapnet.forfalt);
    assert!(!gjenaapnet.ferdig);
    assert_eq!(gjenaapnet.attempt_no, 1, "forsøkstelleren fortsetter");
    assert_eq!(gjenaapnet.forsok, 1, "forsøkshistorikken beholdes");
    assert_eq!(
        gjenaapnet.siste_detalj.as_deref(),
        Some("sikri_unknown_user")
    );

    for (id, status) in [(venter, "retry_venter"), (blokkert, "blokkert")] {
        let rad = rad(&f.pool, id).await;
        assert_eq!(rad.status, status, "statusen beholdes ved fremskynding");
        assert!(rad.forfalt);
    }
    for (id, status) in [(ok, "ok"), (kjorer, "kjorer"), (sendt, "sendt")] {
        let rad = rad(&f.pool, id).await;
        assert_eq!(rad.status, status);
        assert!(!rad.forfalt, "{status} skal ikke røres");
    }
    assert_eq!(
        rad(&f.pool, annen).await.status,
        "feilet",
        "bare de valgte kommandoene frigis"
    );
}

#[tokio::test]
async fn uavklart_utfall_paa_saken_sperrer_uten_endringer() {
    let f = start().await;
    sak(&f.pool, SAK, Uuid::new_v4()).await;
    sak(&f.pool, ANNEN_SAK, Uuid::new_v4()).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", Some(SAK), true).await;
    let feilet = operasjon(&f.pool, RETRY, SAK, "feilet").await;
    // Uavklart operasjon fra en annen kommando på samme sak, allerede varslet.
    command(&f.pool, ANNEN_COMMAND, "avslutt_sak", Some(SAK), true).await;
    let uavklart = operasjon(&f.pool, ANNEN_COMMAND, SAK, "krever_avklaring").await;
    sqlx::query("UPDATE operasjon SET avklaring_varslet_at = now() WHERE operasjon_id = $1")
        .bind(uavklart)
        .execute(&f.pool)
        .await
        .unwrap();

    let feil = repo(&f.pool)
        .frigi(&[SkuffenSakId(SAK), SkuffenSakId(ANNEN_SAK)], &[RETRY])
        .await
        .unwrap_err();

    assert!(
        matches!(feil, FrigivelseFeil::SakHarUavklartUtfall(saker) if saker == vec![SkuffenSakId(SAK)])
    );
    assert_eq!(rad(&f.pool, feilet).await.status, "feilet");
    assert_eq!(rad(&f.pool, uavklart).await.status, "krever_avklaring");
}

#[tokio::test]
async fn laast_operasjon_gir_opptatt_uten_aa_vente() {
    let f = start().await;
    sak(&f.pool, SAK, Uuid::new_v4()).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", Some(SAK), true).await;
    let feilet = operasjon(&f.pool, RETRY, SAK, "feilet").await;
    let annen = operasjon(&f.pool, RETRY, SAK, "kjorer").await;

    // Executoren holder en rad på saken i en åpen transaksjon.
    let mut executor = f.pool.begin().await.unwrap();
    sqlx::query("UPDATE operasjon SET siste_detalj = 'pågår' WHERE operasjon_id = $1")
        .bind(annen)
        .execute(&mut *executor)
        .await
        .unwrap();

    let feil = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        repo(&f.pool).frigi(&[SkuffenSakId(SAK)], &[RETRY]),
    )
    .await
    .expect("admin venter ikke på låsen")
    .unwrap_err();

    assert!(matches!(feil, FrigivelseFeil::Opptatt));
    executor.commit().await.unwrap();
    assert_eq!(rad(&f.pool, feilet).await.status, "feilet");
}

#[tokio::test]
async fn frigivelse_uten_kommandoer_kontrollerer_bare_saken() {
    let f = start().await;
    sak(&f.pool, SAK, Uuid::new_v4()).await;
    command(&f.pool, AVVIST, "avslutt_sak", Some(SAK), false).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", Some(SAK), true).await;
    let feilet = operasjon(&f.pool, RETRY, SAK, "feilet").await;
    let for_frigivelse = rad(&f.pool, feilet).await;

    repo(&f.pool)
        .frigi(&[SkuffenSakId(SAK)], &[])
        .await
        .unwrap();

    assert_eq!(rad(&f.pool, feilet).await, for_frigivelse);

    operasjon(&f.pool, RETRY, SAK, "krever_avklaring").await;
    let feil = repo(&f.pool)
        .frigi(&[SkuffenSakId(SAK)], &[])
        .await
        .unwrap_err();
    assert!(
        matches!(feil, FrigivelseFeil::SakHarUavklartUtfall(saker) if saker == vec![SkuffenSakId(SAK)])
    );
    assert_eq!(rad(&f.pool, feilet).await, for_frigivelse);
}

fn sett_saksansvarlig_plan(command_id: Uuid, saksbehandler: &str) -> Dekomponeringsplan {
    Dekomponeringsplan {
        command_id,
        sak: SakRad {
            sak_id: SkuffenSakId(SAK),
            client_reference: None,
            arkiv_id: Some("2026/1".to_string()),
            attributter: None,
            oensket_saksansvarlig: Some((saksbehandler.to_string(), "42".to_string())),
        },
        journalpost: None,
        dokumenter: Vec::new(),
        operasjoner: vec![OperasjonRad {
            operasjon_id: OperasjonId(Uuid::now_v7()),
            operasjonstype: Operasjonstype::SettSaksansvarlig,
            entitet_id: EntitetId::Sak(SkuffenSakId(SAK)),
            sak_id: SkuffenSakId(SAK),
        }],
    }
}

#[tokio::test]
async fn replay_av_dekomponert_kommando_overskriver_ikke_fakta() {
    let f = start().await;
    sak(&f.pool, SAK, Uuid::new_v4()).await;
    command(&f.pool, RETRY, "sett_saksansvarlig", None, false).await;
    let operasjoner = PostgresOperasjonRepository::new(f.pool.clone());

    let forste = operasjoner
        .lagre_dekomponering(sett_saksansvarlig_plan(RETRY, "Z1"))
        .await
        .unwrap();
    // En nyere kommando endrer ønsket saksansvarlig.
    sqlx::query("UPDATE sak_tilstand SET oensket_saksansvarlig_id = 'Z2' WHERE sak_id = $1")
        .bind(SAK)
        .execute(&f.pool)
        .await
        .unwrap();
    let replay = operasjoner
        .lagre_dekomponering(sett_saksansvarlig_plan(RETRY, "Z1"))
        .await
        .unwrap();

    assert_eq!(forste.nye_operasjoner, 1);
    assert_eq!(replay.nye_operasjoner, 0);
    let (oensket, sak_id, antall): (String, Option<Uuid>, i64) = sqlx::query_as(
        "SELECT s.oensket_saksansvarlig_id, c.sak_id,
                (SELECT count(*) FROM operasjon WHERE command_id = c.command_id)
         FROM sak_tilstand s, command c
         WHERE s.sak_id = $1 AND c.command_id = $2",
    )
    .bind(SAK)
    .bind(RETRY)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    assert_eq!(oensket, "Z2", "replay med gammel payload skal ikke vinne");
    assert_eq!(sak_id, Some(SAK), "dekomponeringen fullfører koblingen");
    assert_eq!(antall, 1);
}
