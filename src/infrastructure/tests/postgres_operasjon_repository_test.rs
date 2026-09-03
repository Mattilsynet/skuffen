//! At-most-once-grensen (SKU-0016 R4) og forfallsklokka (SKU-0020).
//!
//! `fullfor_ok` skal skrive statusovergangen og faktaoppdateringen i **én**
//! transaksjon. Splittes den i to commits, kan operasjonen bli `ok` mens
//! faktaene mangler — og da er et vellykket arkivskriv usynlig for oss, med
//! duplikat i arkivet ved neste forsøk som resultat.
//!
//! Disse testene feiler hvis noen splitter transaksjonen.

use std::collections::HashSet;

use application::command::ports::operasjon_port::{
    CommandOutcome, Faktaoppdatering, OperasjonRepository,
};
use domain::eksekvering::operasjon::{OperasjonId, Operasjonsstatus};
use infrastructure::command::adapter::postgres_operasjon_repository::{
    BLOKKERT_SJEKKFREKVENS, PostgresOperasjonRepository,
};
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
        .max_connections(4)
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

/// Legger inn én blokkert operasjon med egen entitet, egen forfallstid og
/// egen opprettelsestid — alt regnet ut på databasens klokke.
async fn seed_blokkert(
    pool: &DbPool,
    nummer: u128,
    forfall_om_sekunder: f64,
    opprettet_for_sekunder: f64,
) -> OperasjonId {
    let entitet_id = Uuid::from_u128(0x2000 + nummer);
    let operasjon_id = Uuid::from_u128(0x1000 + nummer);

    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference) VALUES ($1, 'dokument', $2)",
    )
    .bind(entitet_id)
    .bind(Uuid::from_u128(0x3000 + nummer))
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO operasjon
             (operasjon_id, command_id, operasjonstype, entitet_id, sak_id,
              status, neste_forsok_at, created_at)
         VALUES ($1, $2, 'render_dokument', $3, $4, 'blokkert',
                 now() + make_interval(secs => $5),
                 now() - make_interval(secs => $6))",
    )
    .bind(operasjon_id)
    .bind(COMMAND_ID)
    .bind(entitet_id)
    .bind(SAK_ID)
    .bind(forfall_om_sekunder)
    .bind(opprettet_for_sekunder)
    .execute(pool)
    .await
    .unwrap();

    OperasjonId(operasjon_id)
}

/// Sekunder til forfall, målt mot databasens `now()`.
async fn sekunder_til_forfall(pool: &DbPool, operasjon_id: OperasjonId) -> f64 {
    sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (neste_forsok_at - now()))::float8
         FROM operasjon WHERE operasjon_id = $1",
    )
    .bind(Uuid::from(operasjon_id))
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

// ---------------------------------------------------------------------------
// Executor-låsen (SKU-0016 R5, single executor)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn andre_instans_far_ikke_laasen_mens_forste_holder_den() {
    let fixture = start().await;
    let a = PostgresOperasjonRepository::new(fixture.pool.clone());
    let b = PostgresOperasjonRepository::new(fixture.pool.clone());

    let lease = a
        .try_acquire_executor_lock("a")
        .await
        .unwrap()
        .expect("første instans blir leder");

    assert!(
        b.try_acquire_executor_lock("b").await.unwrap().is_none(),
        "to instanser kan ikke være executor samtidig"
    );

    drop(lease);
}

#[tokio::test]
async fn laasen_slippes_naar_leasen_droppes() {
    let fixture = start().await;
    let a = PostgresOperasjonRepository::new(fixture.pool.clone());
    let b = PostgresOperasjonRepository::new(fixture.pool.clone());

    let lease = a.try_acquire_executor_lock("a").await.unwrap().unwrap();
    drop(lease);

    // Connection-en er detached, så droppet lukker sessionen. Postgres kan
    // bruke et øyeblikk på å rive den ned.
    let mut overtok = false;
    for _ in 0..50 {
        if b.try_acquire_executor_lock("b").await.unwrap().is_some() {
            overtok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    assert!(
        overtok,
        "en ny instans må kunne overta lederskapet når den gamle slipper det"
    );
}

#[tokio::test]
async fn laasen_overlever_at_poolen_brukes_til_annet_arbeid() {
    let fixture = start().await;
    let a = PostgresOperasjonRepository::new(fixture.pool.clone());
    let b = PostgresOperasjonRepository::new(fixture.pool.clone());

    let _lease = a.try_acquire_executor_lock("a").await.unwrap().unwrap();

    // Bruk opp poolen på vanlige spørringer.
    for _ in 0..20 {
        let _: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    }

    assert!(
        b.try_acquire_executor_lock("b").await.unwrap().is_none(),
        "låsen skal fortsatt holdes etter at poolen har servet annet arbeid"
    );
}
// Måler om detach() reduserer poolens kapasitet permanent.
#[tokio::test]
async fn detach_stjeler_ikke_plass_i_poolen() {
    let fixture = start().await;
    // Egen pool med kjent, liten kapasitet.
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect_with(fixture.connect_options.clone())
        .await
        .unwrap();

    let detached = pool.acquire().await.unwrap().detach();

    // Hvis detach frigjør plassen, skal vi fortsatt få 2 samtidige.
    let a = pool.acquire().await.expect("første plass");
    let b = pool
        .acquire()
        .await
        .expect("andre plass — detach skal ha frigjort permit");

    drop((a, b, detached));
}

// ---------------------------------------------------------------------------
// Lederskap ved utrulling
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ny_instans_overtar_lederskapet_naar_den_gamle_slipper_det() {
    let fixture = start().await;
    let gammel = PostgresOperasjonRepository::new(fixture.pool.clone());
    let ny = PostgresOperasjonRepository::new(fixture.pool.clone());

    let lease = gammel.try_acquire_executor_lock("gammel").await.unwrap();
    assert!(lease.is_some(), "gammel instans er leder");

    // Ny instans prøver mens den gamle lever.
    assert!(
        ny.try_acquire_executor_lock("ny").await.unwrap().is_none(),
        "ny instans må vente mens den gamle er leder"
    );

    // Gammel instans avslutter.
    drop(lease);

    // Ny instans skal kunne overta ved neste forsøk.
    let mut overtok = false;
    for _ in 0..50 {
        if ny.try_acquire_executor_lock("ny").await.unwrap().is_some() {
            overtok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(overtok, "ny instans må overta når den gamle slipper låsen");
}

// ---------------------------------------------------------------------------
// Forfallsklokka (SKU-0020 R2, R3, R4)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn forfalt_blokkert_operasjon_plukkes() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());
    let forfalt = seed_blokkert(&fixture.pool, 1, -1.0, 0.0).await;

    let plukket = repo
        .hent_neste_kjorbare()
        .await
        .unwrap()
        .expect("en forfalt blokkert operasjon er kjørbar");

    assert_eq!(
        plukket.operasjon_id, forfalt,
        "blokkering er en beslutning executoren tar på nytt, ikke en cache"
    );
}

#[tokio::test]
async fn blokkert_operasjon_med_frist_i_fremtiden_plukkes_ikke() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());
    seed_blokkert(&fixture.pool, 1, 3600.0, 0.0).await;

    assert!(
        repo.hent_neste_kjorbare().await.unwrap().is_none(),
        "fristen bounder kostnaden ved permanent blokkerte rader"
    );
}

#[tokio::test]
async fn rekkefolgen_folger_forfall_ikke_opprettelsestid() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    // Eldst, men forfaller sist.
    seed_blokkert(&fixture.pool, 1, -1.0, 3600.0).await;
    // Nyest, men forfalt for lengst.
    let forst = seed_blokkert(&fixture.pool, 2, -600.0, 1.0).await;

    let plukket = repo.hent_neste_kjorbare().await.unwrap().unwrap();

    assert_eq!(
        plukket.operasjon_id, forst,
        "ORDER BY neste_forsok_at er det som roterer gjennom køen"
    );
}

#[tokio::test]
async fn marker_blokkert_skyver_fristen_en_fast_sjekkfrekvens_frem() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());
    let blokkert = seed_blokkert(&fixture.pool, 1, -1.0, 0.0).await;

    repo.marker_blokkert(blokkert, None, "fortsatt blokkert")
        .await
        .unwrap();

    let sekunder = sekunder_til_forfall(&fixture.pool, blokkert).await;
    let frekvens = BLOKKERT_SJEKKFREKVENS.as_secs_f64();
    assert!(
        sekunder > frekvens - 5.0 && sekunder <= frekvens,
        "fristen skal skyves {frekvens} sekunder frem, ikke {sekunder}"
    );
}

#[tokio::test]
async fn fullfor_ok_setter_blokkerte_sosken_paa_samme_sak_forfalt() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());
    let sosken = seed_blokkert(&fixture.pool, 1, 3600.0, 0.0).await;

    repo.fullfor_ok(
        OperasjonId(OPERASJON_ID),
        1,
        Faktaoppdatering::SakOpprettet {
            arkiv_id: "2026/997".to_string(),
        },
    )
    .await
    .unwrap();

    assert!(
        sekunder_til_forfall(&fixture.pool, sosken).await <= 0.0,
        "søskenvekkingen er latenshintet som gjør at en journalpostkommando \
         ruller gjennom i ett strekk"
    );
}

#[tokio::test]
async fn alle_blokkerte_rader_evalueres_selv_over_gammel_grense() {
    const ANTALL: u128 = 250;

    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    for nummer in 1..=ANTALL {
        seed_blokkert(&fixture.pool, nummer, -1.0, nummer as f64).await;
    }

    // Hver runde skyver fristen frem, så ingen rad kan plukkes to ganger.
    let mut sett = HashSet::new();
    while let Some(op) = repo.hent_neste_kjorbare().await.unwrap() {
        assert!(
            sett.insert(op.operasjon_id),
            "en rad ble plukket to ganger i samme gjennomgang"
        );
        repo.marker_blokkert(op.operasjon_id, None, "fortsatt blokkert")
            .await
            .unwrap();
    }

    assert_eq!(
        sett.len(),
        ANTALL as usize,
        "de eldste blokkerte radene skal ikke kunne sulte ut resten av køen"
    );
}

/// Folden er monoton. `KreverAvklaring` er en ikke-terminal mellomstasjon som
/// kan bli `Fullfort` etter manuell opprydding, men et terminalt `Feilet` kan
/// aldri trekkes tilbake av et uavklart søsken.
#[tokio::test]
async fn krever_avklaring_folder_til_eget_utfall_men_taper_for_feilet() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    // Den seedede operasjonen står i `sendt`; recovery gjør den uavklart.
    repo.gjenopprett_etter_restart().await.unwrap();
    assert_eq!(
        repo.hent_command_outcome(COMMAND_ID).await.unwrap(),
        CommandOutcome::KreverAvklaring
    );

    // Et søsken som feiler terminalt overtar utfallet.
    let entitet_id = Uuid::from_u128(0x2999);
    sqlx::query(
        "INSERT INTO entitet (skuffen_id, entitet_type, client_reference) VALUES ($1, 'dokument', $2)",
    )
    .bind(entitet_id)
    .bind(Uuid::from_u128(0x3999))
    .execute(&fixture.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO operasjon
             (operasjon_id, command_id, operasjonstype, entitet_id, sak_id, status, ferdig_at)
         VALUES ($1, $2, 'render_dokument', $3, $4, 'feilet', now())",
    )
    .bind(Uuid::from_u128(0x1999))
    .bind(COMMAND_ID)
    .bind(entitet_id)
    .bind(SAK_ID)
    .execute(&fixture.pool)
    .await
    .unwrap();

    assert_eq!(
        repo.hent_command_outcome(COMMAND_ID).await.unwrap(),
        CommandOutcome::Feilet,
        "et terminalt Feilet kan ikke trekkes tilbake"
    );
}

/// Recovery markerer ikke varselet selv. Kolonnen skrives først når
/// publiseringen har lyktes, så en krasj imellom gir duplikat, ikke tap.
#[tokio::test]
async fn avklaringsvarsel_hentes_til_det_er_markert() {
    let fixture = start().await;
    seed(&fixture.pool).await;
    let repo = PostgresOperasjonRepository::new(fixture.pool.clone());

    repo.gjenopprett_etter_restart().await.unwrap();
    assert_eq!(repo.hent_krever_avklaring().await.unwrap().len(), 1);

    repo.marker_avklaring_varslet(OperasjonId(OPERASJON_ID))
        .await
        .unwrap();

    assert!(
        repo.hent_krever_avklaring().await.unwrap().is_empty(),
        "en markert rad skal ikke varsles på nytt ved neste oppstart"
    );
}
