//! Admin read-projectionen mot ekte PostgreSQL.
//!
//! Testene låser tre ting: at projectionen viser lagret state uten å
//! revalidere den, at ordering er deterministisk, og at hvert oppslag kommer
//! fra ett `REPEATABLE READ READ ONLY`-snapshot.

use application::admin::model::{AdminCommandUtfall, AdminSakNokkel};
use application::admin::ports::admin_read_repository::AdminReadRepository;
use chrono::{DateTime, TimeZone, Utc};
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::EntitetId;
use infrastructure::admin::adapter::postgres_admin_read_repository::PostgresAdminReadRepository;
use lib_sql::database_config::DbPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

/// Speiler `SNAPSHOT_BEGIN` i adapteren.
const SNAPSHOT_BEGIN: &str = "BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY";

struct Fixture {
    _container: testcontainers::ContainerAsync<Postgres>,
    pool: DbPool,
    connect_options: PgConnectOptions,
}

/// Bygges fra deler, ikke som URL-streng — testcontainers-URL-er ser ut som
/// hardkodede credentials for hemmelighetsskanneren.
fn connect_options(port: u16) -> PgConnectOptions {
    PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("postgres")
        .password("postgres")
        .database("postgres")
}

async fn start() -> Fixture {
    let container = Postgres::default().start().await.expect("postgres startet");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("postgres port");
    let connect_options = connect_options(port);

    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect_with(connect_options.clone())
        .await
        .expect("koblet til postgres");

    infrastructure::database::setup::run_migrations(&pool)
        .await
        .expect("migrasjoner kjørte");

    Fixture {
        _container: container,
        pool,
        connect_options,
    }
}

fn tid(sekund: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 27, 10, 0, sekund).unwrap()
}

// ---------------------------------------------------------------------------
// Seeding
// ---------------------------------------------------------------------------

async fn seed_command(pool: &DbPool, command_id: Uuid, command_type: &str) {
    sqlx::query("INSERT INTO command (command_id, command_type) VALUES ($1, $2)")
        .bind(command_id)
        .bind(command_type)
        .execute(pool)
        .await
        .unwrap();
}

async fn seed_entitet(
    pool: &DbPool,
    skuffen_id: Uuid,
    entitet_type: &str,
    client_reference: Option<Uuid>,
    arkiv_id: Option<&str>,
) {
    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference, arkiv_id)
         VALUES ($1, $2::entitet_type, $3, $4)",
    )
    .bind(skuffen_id)
    .bind(entitet_type)
    .bind(client_reference)
    .bind(arkiv_id)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_sak_tilstand(
    pool: &DbPool,
    sak_id: Uuid,
    command_id: Uuid,
    sakstittel: Option<&str>,
    opprettelse_saksbehandler: Option<(&str, &str)>,
    oensket_saksansvarlig: Option<(&str, &str)>,
    naavaerende_saksansvarlig: Option<(&str, &str)>,
) {
    sqlx::query(
        "INSERT INTO sak_tilstand (
             sak_id, tilstand, sakstittel, arkivdel, ordningsverdi,
             saksbehandler_id, saksbehandler_enhet,
             oensket_saksansvarlig_id, oensket_saksansvarlig_enhet,
             naavaerende_saksansvarlig_id, naavaerende_saksansvarlig_enhet,
             opprettet_av_command_id)
         VALUES ($1, 'opprettet', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(sak_id)
    .bind(sakstittel)
    .bind(sakstittel.map(|_| "tilsynsdivisjonene"))
    .bind(sakstittel.map(|_| "123"))
    .bind(opprettelse_saksbehandler.map(|(id, _)| id))
    .bind(opprettelse_saksbehandler.map(|(_, enhet)| enhet))
    .bind(oensket_saksansvarlig.map(|(id, _)| id))
    .bind(oensket_saksansvarlig.map(|(_, enhet)| enhet))
    .bind(naavaerende_saksansvarlig.map(|(id, _)| id))
    .bind(naavaerende_saksansvarlig.map(|(_, enhet)| enhet))
    .bind(command_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_journalpost(
    pool: &DbPool,
    journalpost_id: Uuid,
    sak_id: Uuid,
    command_id: Uuid,
    saksbehandler: (&str, &str),
    korrespondanseparter: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO journalpost_tilstand (
             journalpost_id, sak_id, tilstand, journalposttype, med_utsending,
             tittel, dokument_dato, saksbehandler_id, saksbehandler_enhet,
             korrespondanseparter, kildesystem, opprettet_av_command_id, created_at)
         VALUES ($1, $2, 'opprettet', 'X', false, 'Internt notat', '2026-01-01',
                 $3, $4, $5, 'skuffen-test', $6, $7)",
    )
    .bind(journalpost_id)
    .bind(sak_id)
    .bind(saksbehandler.0)
    .bind(saksbehandler.1)
    .bind(korrespondanseparter)
    .bind(command_id)
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_dokument(
    pool: &DbPool,
    dokument_id: Uuid,
    journalpost_id: Uuid,
    command_id: Uuid,
    rekkefolge: i32,
    dokument_referanse: Option<Uuid>,
    mal_referanse: Option<Uuid>,
    felter: Option<serde_json::Value>,
    rendered_dokument_referanse: Option<Uuid>,
) {
    sqlx::query(
        "INSERT INTO dokument_tilstand (
             dokument_id, journalpost_id, tilstand, rekkefolge, er_hoveddokument,
             tittel, filtype, dokument_referanse, mal_referanse, felter,
             rendered_dokument_referanse, opprettet_av_command_id)
         VALUES ($1, $2, 'klar', $3, $4, 'Dokument', 'PDF', $5, $6, $7, $8, $9)",
    )
    .bind(dokument_id)
    .bind(journalpost_id)
    .bind(rekkefolge)
    .bind(rekkefolge == 0)
    .bind(dokument_referanse)
    .bind(mal_referanse)
    .bind(felter)
    .bind(rendered_dokument_referanse)
    .bind(command_id)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_operasjon(
    pool: &DbPool,
    operasjon_id: Uuid,
    command_id: Uuid,
    operasjonstype: &str,
    entitet_id: Uuid,
    sak_id: Uuid,
    status: &str,
    created_at: DateTime<Utc>,
) {
    let terminal = matches!(status, "ok" | "feilet");
    sqlx::query(
        "INSERT INTO operasjon (
             operasjon_id, command_id, operasjonstype, entitet_id, sak_id, status,
             attempt_no, siste_detalj, ferdig_at, created_at)
         VALUES ($1, $2, $3, $4, $5, $6::operasjon_status, 2, 'siste detalj', $7, $8)",
    )
    .bind(operasjon_id)
    .bind(command_id)
    .bind(operasjonstype)
    .bind(entitet_id)
    .bind(sak_id)
    .bind(status)
    .bind(terminal.then(|| tid(30)))
    .bind(created_at)
    .execute(pool)
    .await
    .unwrap();
}

/// Én sak med journalpost, to dokumenter og operasjoner.
struct Scenario {
    command_id: Uuid,
    sak_id: Uuid,
    sak_client_reference: Uuid,
    arkiv_id: String,
    journalpost_id: Uuid,
    hoveddokument_id: Uuid,
    vedlegg_id: Uuid,
}

async fn seed_scenario(pool: &DbPool) -> Scenario {
    let scenario = Scenario {
        command_id: Uuid::new_v4(),
        sak_id: Uuid::new_v4(),
        sak_client_reference: Uuid::new_v4(),
        arkiv_id: "2026/12345".to_string(),
        journalpost_id: Uuid::new_v4(),
        hoveddokument_id: Uuid::new_v4(),
        vedlegg_id: Uuid::new_v4(),
    };

    seed_command(
        pool,
        scenario.command_id,
        "opprett_internt_notat_journalpost",
    )
    .await;
    seed_entitet(
        pool,
        scenario.sak_id,
        "sak",
        Some(scenario.sak_client_reference),
        Some(&scenario.arkiv_id),
    )
    .await;
    seed_sak_tilstand(
        pool,
        scenario.sak_id,
        scenario.command_id,
        Some("Tilsynssak"),
        Some(("A", "A-enhet")),
        Some(("B", "B-enhet")),
        Some(("C", "C-enhet")),
    )
    .await;

    seed_entitet(
        pool,
        scenario.journalpost_id,
        "journalpost",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_journalpost(
        pool,
        scenario.journalpost_id,
        scenario.sak_id,
        scenario.command_id,
        ("D", "D-enhet"),
        Some(serde_json::json!([
            { "rolle": "mottaker", "navn": "Testmottaker", "parttype": "virksomhet" }
        ])),
        tid(1),
    )
    .await;

    seed_entitet(
        pool,
        scenario.hoveddokument_id,
        "dokument",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_dokument(
        pool,
        scenario.hoveddokument_id,
        scenario.journalpost_id,
        scenario.command_id,
        0,
        None,
        Some(Uuid::from_u128(0x_a1)),
        Some(serde_json::json!(["{{saksnummer}}"])),
        Some(Uuid::from_u128(0x_df)),
    )
    .await;

    seed_entitet(
        pool,
        scenario.vedlegg_id,
        "dokument",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_dokument(
        pool,
        scenario.vedlegg_id,
        scenario.journalpost_id,
        scenario.command_id,
        1,
        Some(Uuid::from_u128(0x_be)),
        None,
        None,
        None,
    )
    .await;

    seed_operasjon(
        pool,
        Uuid::new_v4(),
        scenario.command_id,
        "opprett_journalpost",
        scenario.journalpost_id,
        scenario.sak_id,
        "ok",
        tid(1),
    )
    .await;
    seed_operasjon(
        pool,
        Uuid::new_v4(),
        scenario.command_id,
        "legg_til_vedlegg",
        scenario.vedlegg_id,
        scenario.sak_id,
        "krever_avklaring",
        tid(2),
    )
    .await;

    scenario
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

#[tokio::test]
async fn command_returnerer_alle_operasjoner_i_deterministisk_rekkefolge() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;
    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let command = repo
        .hent_command(scenario.command_id)
        .await
        .unwrap()
        .expect("command finnes");

    assert_eq!(command.command_id, scenario.command_id);
    assert_eq!(
        command.command_type,
        "opprett_internt_notat_journalpost".to_string()
    );
    assert_eq!(command.operasjoner.len(), 2);
    assert_eq!(command.operasjoner[0].operasjonstype, "opprett_journalpost");
    assert_eq!(command.operasjoner[1].operasjonstype, "legg_til_vedlegg");
    assert_eq!(command.operasjoner[0].status, "ok");
    assert_eq!(command.operasjoner[0].attempt_no, 2);
    assert_eq!(
        command.operasjoner[0].siste_detalj.as_deref(),
        Some("siste detalj")
    );
    assert!(command.operasjoner[0].ferdig_at.is_some());
    assert_eq!(
        command.operasjoner[0].entitet.skuffen_id,
        EntitetId::Journalpost(scenario.journalpost_id.into())
    );
    assert_eq!(command.utled_utfall(), AdminCommandUtfall::KreverAvklaring);
}

#[tokio::test]
async fn command_uten_operasjoner_er_uavklart_ogsaa_naar_den_er_dekomponert() {
    let fixture = start().await;

    let uten_dekomponering = Uuid::new_v4();
    seed_command(&fixture.pool, uten_dekomponering, "opprett_sak").await;

    let inkonsistent = Uuid::new_v4();
    seed_command(&fixture.pool, inkonsistent, "opprett_sak").await;
    sqlx::query("UPDATE command SET dekomponert_at = now() WHERE command_id = $1")
        .bind(inkonsistent)
        .execute(&fixture.pool)
        .await
        .unwrap();

    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let uten = repo
        .hent_command(uten_dekomponering)
        .await
        .unwrap()
        .expect("command finnes");
    assert!(uten.dekomponert_at.is_none());
    assert!(uten.operasjoner.is_empty());
    assert_eq!(uten.utled_utfall(), AdminCommandUtfall::Uavklart);

    let inkonsistent = repo
        .hent_command(inkonsistent)
        .await
        .unwrap()
        .expect("command finnes");
    assert!(inkonsistent.dekomponert_at.is_some());
    assert!(inkonsistent.operasjoner.is_empty());
    assert_eq!(inkonsistent.utled_utfall(), AdminCommandUtfall::Uavklart);

    assert!(repo.hent_command(Uuid::new_v4()).await.unwrap().is_none());
}

#[tokio::test]
async fn feilet_har_prioritet_over_krever_avklaring() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;
    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    assert_eq!(
        repo.hent_command(scenario.command_id)
            .await
            .unwrap()
            .unwrap()
            .utled_utfall(),
        AdminCommandUtfall::KreverAvklaring
    );

    seed_operasjon(
        &fixture.pool,
        Uuid::new_v4(),
        scenario.command_id,
        "journalfoer",
        scenario.journalpost_id,
        scenario.sak_id,
        "feilet",
        tid(3),
    )
    .await;

    assert_eq!(
        repo.hent_command(scenario.command_id)
            .await
            .unwrap()
            .unwrap()
            .utled_utfall(),
        AdminCommandUtfall::Feilet
    );
}

// ---------------------------------------------------------------------------
// Sak
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sak_kan_slaas_opp_med_alle_tre_noklene() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;
    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let per_skuffen_id = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(scenario.sak_id)))
        .await
        .unwrap()
        .expect("sak finnes");
    let per_client_reference = repo
        .hent_sak(AdminSakNokkel::ClientReference(
            scenario.sak_client_reference,
        ))
        .await
        .unwrap()
        .expect("sak finnes");
    let per_arkiv_id = repo
        .hent_sak(AdminSakNokkel::ArkivId(scenario.arkiv_id.clone()))
        .await
        .unwrap()
        .expect("sak finnes");

    assert_eq!(per_skuffen_id, per_client_reference);
    assert_eq!(per_skuffen_id, per_arkiv_id);
    assert_eq!(
        per_skuffen_id.identitet.skuffen_id,
        EntitetId::Sak(SkuffenSakId(scenario.sak_id))
    );

    assert!(
        repo.hent_sak(AdminSakNokkel::ArkivId("2026/ukjent".to_string()))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn identity_only_sak_er_success_med_fakta_none() {
    let fixture = start().await;
    let sak_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();
    seed_entitet(&fixture.pool, sak_id, "sak", Some(client_reference), None).await;

    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());
    let sak = repo
        .hent_sak(AdminSakNokkel::ClientReference(client_reference))
        .await
        .unwrap()
        .expect("identitet finnes");

    assert!(sak.fakta.is_none());
    assert_eq!(sak.identitet.client_reference, Some(client_reference));
    assert!(sak.operasjoner.is_empty());
}

#[tokio::test]
async fn sak_tilstand_med_null_attributter_gir_fakta_some() {
    let fixture = start().await;
    let command_id = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    seed_command(&fixture.pool, command_id, "opprett_sak").await;
    seed_entitet(&fixture.pool, sak_id, "sak", Some(client_reference), None).await;
    seed_sak_tilstand(&fixture.pool, sak_id, command_id, None, None, None, None).await;

    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());
    let sak = repo
        .hent_sak(AdminSakNokkel::ClientReference(client_reference))
        .await
        .unwrap()
        .expect("sak finnes");

    let fakta = sak.fakta.expect("fakta finnes selv med NULL-attributter");
    assert_eq!(fakta.tilstand, "opprettet");
    assert!(fakta.sakstittel.is_none());
    assert!(fakta.opprettelse_saksbehandler_id.is_none());
    assert!(fakta.journalposter.is_empty());
}

#[tokio::test]
async fn full_sak_inkluderer_barn_provenance_og_mediareferanser() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;
    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let sak = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(scenario.sak_id)))
        .await
        .unwrap()
        .expect("sak finnes");

    let fakta = sak.fakta.expect("fakta finnes");
    assert_eq!(fakta.opprettet_av_command_id, scenario.command_id);
    assert_eq!(fakta.journalposter.len(), 1);

    let journalpost = &fakta.journalposter[0];
    assert_eq!(journalpost.sak_id, SkuffenSakId(scenario.sak_id));
    assert_eq!(journalpost.kildesystem.as_deref(), Some("skuffen-test"));
    assert_eq!(journalpost.opprettet_av_command_id, scenario.command_id);
    let parter = journalpost
        .korrespondanseparter
        .as_ref()
        .expect("korrespondanseparter finnes");
    assert_eq!(parter.len(), 1);
    assert_eq!(parter[0].rolle, "mottaker");
    assert_eq!(parter[0].parttype.as_deref(), Some("virksomhet"));
    assert!(parter[0].postnummer.is_none());

    assert_eq!(journalpost.dokumenter.len(), 2);
    assert!(journalpost.dokumenter[0].er_hoveddokument);
    assert_eq!(journalpost.dokumenter[0].rekkefolge, 0);
    assert_eq!(journalpost.dokumenter[1].rekkefolge, 1);
}

#[tokio::test]
async fn saksbehandlerkontekstene_holdes_adskilt() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;
    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let sak = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(scenario.sak_id)))
        .await
        .unwrap()
        .unwrap();
    let fakta = sak.fakta.expect("fakta finnes");

    assert_eq!(fakta.opprettelse_saksbehandler_id.as_deref(), Some("A"));
    assert_eq!(
        fakta.opprettelse_saksbehandler_enhet.as_deref(),
        Some("A-enhet")
    );
    assert_eq!(fakta.oensket_saksansvarlig_id.as_deref(), Some("B"));
    assert_eq!(fakta.naavaerende_saksansvarlig_id.as_deref(), Some("C"));
    assert_eq!(
        fakta.journalposter[0].saksbehandler_id.as_deref(),
        Some("D")
    );
    assert_eq!(
        fakta.journalposter[0].saksbehandler_enhet.as_deref(),
        Some("D-enhet")
    );
}

#[tokio::test]
async fn sakens_operasjonssammendrag_har_command_id_for_alle_operasjoner() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;

    let annen_command = Uuid::new_v4();
    seed_command(&fixture.pool, annen_command, "avslutt_sak").await;
    seed_operasjon(
        &fixture.pool,
        Uuid::new_v4(),
        annen_command,
        "avslutt_sak",
        scenario.sak_id,
        scenario.sak_id,
        "klar",
        tid(9),
    )
    .await;

    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());
    let sak = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(scenario.sak_id)))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(sak.operasjoner.len(), 3);
    assert!(
        sak.operasjoner
            .iter()
            .any(|operasjon| operasjon.command_id == annen_command)
    );
    assert!(
        sak.operasjoner
            .iter()
            .any(|operasjon| operasjon.command_id == scenario.command_id)
    );
    assert_eq!(sak.operasjoner[2].operasjonstype, "avslutt_sak");
}

#[tokio::test]
async fn lik_created_at_brytes_deterministisk_med_uuid() {
    let fixture = start().await;
    let command_id = Uuid::new_v4();
    let sak_id = Uuid::new_v4();

    seed_command(&fixture.pool, command_id, "opprett_sak").await;
    seed_entitet(&fixture.pool, sak_id, "sak", Some(Uuid::new_v4()), None).await;
    seed_sak_tilstand(
        &fixture.pool,
        sak_id,
        command_id,
        Some("Tilsynssak"),
        None,
        None,
        None,
    )
    .await;

    // Journalpostene settes inn i ikke-sortert rekkefølge med lik created_at.
    let hoy = Uuid::from_u128(0xffff_0000_0000_0000_0000_0000_0000_0001);
    let lav = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);
    for journalpost_id in [hoy, lav] {
        seed_entitet(
            &fixture.pool,
            journalpost_id,
            "journalpost",
            Some(Uuid::new_v4()),
            None,
        )
        .await;
        seed_journalpost(
            &fixture.pool,
            journalpost_id,
            sak_id,
            command_id,
            ("D", "D-enhet"),
            None,
            tid(5),
        )
        .await;
    }

    let hoy_operasjon = Uuid::from_u128(0xffff_0000_0000_0000_0000_0000_0000_0002);
    let lav_operasjon = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0002);
    for (index, operasjon_id) in [hoy_operasjon, lav_operasjon].into_iter().enumerate() {
        seed_operasjon(
            &fixture.pool,
            operasjon_id,
            command_id,
            if index == 0 {
                "opprett_journalpost"
            } else {
                "journalfoer"
            },
            sak_id,
            sak_id,
            "klar",
            tid(5),
        )
        .await;
    }

    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let sak = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(sak_id)))
        .await
        .unwrap()
        .unwrap();
    let journalposter = sak.fakta.expect("fakta finnes").journalposter;
    assert_eq!(
        journalposter[0].identitet.skuffen_id,
        EntitetId::Journalpost(lav.into())
    );
    assert_eq!(
        journalposter[1].identitet.skuffen_id,
        EntitetId::Journalpost(hoy.into())
    );

    assert_eq!(sak.operasjoner[0].operasjon_id.0, lav_operasjon);
    assert_eq!(sak.operasjoner[1].operasjon_id.0, hoy_operasjon);

    let command = repo.hent_command(command_id).await.unwrap().unwrap();
    assert_eq!(command.operasjoner[0].operasjon_id.0, lav_operasjon);
    assert_eq!(command.operasjoner[1].operasjon_id.0, hoy_operasjon);
}

#[tokio::test]
async fn lagrede_json_og_mediareferanser_bevares_eksakt() {
    let fixture = start().await;
    let command_id = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();

    seed_command(&fixture.pool, command_id, "opprett_utgaaende_journalpost").await;
    seed_entitet(&fixture.pool, sak_id, "sak", Some(Uuid::new_v4()), None).await;
    seed_sak_tilstand(
        &fixture.pool,
        sak_id,
        command_id,
        Some("Tilsynssak"),
        None,
        None,
        None,
    )
    .await;
    seed_entitet(
        &fixture.pool,
        journalpost_id,
        "journalpost",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_journalpost(
        &fixture.pool,
        journalpost_id,
        sak_id,
        command_id,
        ("D", "D-enhet"),
        Some(serde_json::json!([
            {
                "rolle": "utsendingsmottaker",
                "navn": "Testmottaker",
                "id_type": "organisasjonsnummer",
                "id": "999999999",
                "adresse": "Testveien 1",
                "postnummer": "0001",
                "poststed": "Oslo"
            }
        ])),
        tid(1),
    )
    .await;

    let bytes_dokument = Uuid::new_v4();
    let template_dokument = Uuid::new_v4();
    let dokument_referanse = Uuid::from_u128(0x_be);
    let mal_referanse = Uuid::from_u128(0x_a1);
    let rendered = Uuid::from_u128(0x_df);

    seed_entitet(
        &fixture.pool,
        template_dokument,
        "dokument",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_dokument(
        &fixture.pool,
        template_dokument,
        journalpost_id,
        command_id,
        0,
        None,
        Some(mal_referanse),
        Some(serde_json::json!([])),
        Some(rendered),
    )
    .await;

    seed_entitet(
        &fixture.pool,
        bytes_dokument,
        "dokument",
        Some(Uuid::new_v4()),
        None,
    )
    .await;
    seed_dokument(
        &fixture.pool,
        bytes_dokument,
        journalpost_id,
        command_id,
        1,
        Some(dokument_referanse),
        None,
        None,
        None,
    )
    .await;

    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());
    let sak = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(sak_id)))
        .await
        .unwrap()
        .unwrap();
    let journalpost = &sak.fakta.expect("fakta finnes").journalposter[0];

    let part = &journalpost.korrespondanseparter.as_ref().unwrap()[0];
    assert_eq!(part.rolle, "utsendingsmottaker");
    assert_eq!(part.id.as_deref(), Some("999999999"));
    assert_eq!(part.postnummer.as_deref(), Some("0001"));
    assert!(part.parttype.is_none());

    let template = &journalpost.dokumenter[0];
    assert_eq!(template.felter, Some(Vec::new()));
    assert_eq!(template.mal_referanse, Some(mal_referanse));
    assert_eq!(template.rendered_dokument_referanse, Some(rendered));
    assert!(template.dokument_referanse.is_none());

    let bytes = &journalpost.dokumenter[1];
    assert_eq!(bytes.felter, None);
    assert_eq!(bytes.dokument_referanse, Some(dokument_referanse));
    assert!(bytes.mal_referanse.is_none());
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// Blokkerer `journalpost_tilstand` etter at snapshotet er tatt, muterer
/// dokument- og operasjonsrader fra en annen connection, og verifiserer at
/// resten av svaret fortsatt kommer fra det opprinnelige snapshotet.
#[tokio::test]
async fn sak_oppslaget_leser_hele_svaret_fra_ett_snapshot() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;
    let repo = PostgresAdminReadRepository::new(fixture.pool.clone());

    let laser = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(fixture.connect_options.clone())
        .await
        .unwrap();
    let mut lasende_tx = laser.begin().await.unwrap();
    sqlx::query("LOCK TABLE journalpost_tilstand IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *lasende_tx)
        .await
        .unwrap();

    let sak_id = scenario.sak_id;
    let oppslag = tokio::spawn(async move {
        repo.hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(sak_id)))
            .await
    });

    vent_til_blokkert(&fixture.pool, "journalpost_tilstand").await;

    // Snapshotet er allerede tatt av identitets-queryen.
    sqlx::query("UPDATE dokument_tilstand SET tittel = 'endret etter snapshot'")
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE operasjon SET status = 'feilet', ferdig_at = now()")
        .execute(&fixture.pool)
        .await
        .unwrap();

    lasende_tx.rollback().await.unwrap();

    let sak = oppslag.await.unwrap().unwrap().expect("sak finnes");
    let fakta = sak.fakta.expect("fakta finnes");

    for dokument in &fakta.journalposter[0].dokumenter {
        assert_eq!(
            dokument.tittel.as_deref(),
            Some("Dokument"),
            "dokumenter skal komme fra samme snapshot som identiteten"
        );
    }
    assert!(
        sak.operasjoner
            .iter()
            .all(|operasjon| operasjon.status != "feilet"),
        "operasjoner skal komme fra samme snapshot som identiteten"
    );

    // Etter oppslaget er mutasjonene selvsagt synlige.
    let etterpaa: String =
        sqlx::query_scalar("SELECT tittel FROM dokument_tilstand ORDER BY rekkefolge LIMIT 1")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(etterpaa, "endret etter snapshot");
}

/// Én connection er nok for hele sak-oppslaget. Bruker en senere query poolen
/// i stedet for transaksjonen, stopper den her.
#[tokio::test]
async fn sak_oppslaget_bruker_bare_en_connection() {
    let fixture = start().await;
    let scenario = seed_scenario(&fixture.pool).await;

    let enkel_pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(3))
        .connect_with(fixture.connect_options.clone())
        .await
        .unwrap();
    let repo = PostgresAdminReadRepository::new(enkel_pool);

    let sak = repo
        .hent_sak(AdminSakNokkel::SkuffenId(SkuffenSakId(scenario.sak_id)))
        .await
        .unwrap()
        .expect("sak finnes");
    assert!(sak.fakta.is_some());

    let command = repo
        .hent_command(scenario.command_id)
        .await
        .unwrap()
        .expect("command finnes");
    assert_eq!(command.operasjoner.len(), 2);
}

/// Låser SQL-kontrakten adapterens `SNAPSHOT_BEGIN` bygger på.
#[tokio::test]
async fn snapshot_transaksjonen_er_repeatable_read_og_read_only() {
    let fixture = start().await;
    let mut tx = fixture.pool.begin_with(SNAPSHOT_BEGIN).await.unwrap();

    let isolasjon: String = sqlx::query_scalar("SHOW transaction_isolation")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let read_only: String = sqlx::query_scalar("SHOW transaction_read_only")
        .fetch_one(&mut *tx)
        .await
        .unwrap();

    assert_eq!(isolasjon, "repeatable read");
    assert_eq!(read_only, "on");

    tx.commit().await.unwrap();
}

async fn vent_til_blokkert(pool: &DbPool, tabell: &str) {
    for _ in 0..100 {
        let blokkert: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                 SELECT 1 FROM pg_stat_activity
                 WHERE wait_event_type = 'Lock' AND query LIKE '%' || $1 || '%'
             )",
        )
        .bind(tabell)
        .fetch_one(pool)
        .await
        .unwrap();
        if blokkert {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("oppslaget ble aldri blokkert på {tabell}");
}
