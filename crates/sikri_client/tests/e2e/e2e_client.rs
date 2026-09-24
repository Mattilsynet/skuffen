use env::set_var;
use std::env;

/// ### Ende til ende test for hent av sak, dette betinger at sak finnes i ELements for at denne skal fungere.
/// får også verifisert at man får hentet secrets for autentisering av sikri.
///
/// #### Kjør en enkelt test:
/// ```bash
/// cargo test hent_sak_e2e_henter_secrets_og_kaller_api -- --ignored -- nocapture
/// ```
/// #### Kjør alle tester med #[ignore]
/// ```bash
/// cargo test -p sikri_client --test e2e_client -- --ignored --nocapture
/// ```

#[tokio::test]
#[ignore]
async fn hent_sak_e2e_henter_secrets_og_kaller_api() {
    unsafe {
        set_var(
            "BASE_URL_SIKRI",
            "https://services09.elementscloud.no/test/MattilsynetArkivApiTestCloud",
        );
    }
    let _ = env::var("APP_APPLICATION__PROJECT_ID")
        .expect("APP_APPLICATION__PROJECT_ID må være satt (GCP prosjekt-ID)");

    // Miljøvariabler som må være satt
    let project_id = env::var("APP_APPLICATION__PROJECT_ID")
        .expect("APP_APPLICATION__PROJECT_ID må være satt (GCP prosjekt-ID)");
    let base_url =
        env::var("BASE_URL_SIKRI").expect("BASE_URL_SIKRI må være satt til faktisk Sikri-base-URL");

    // Hint: disse varslene sikrer at variablene brukes, men vi trenger ikke selve verdien her
    // project_id brukes indirekte av klienten via get_secret()
    assert!(!project_id.is_empty());
    assert!(!base_url.is_empty());

    let resp = sikri_client::hent_sak("2025/500961", "MATS", false)
        .await
        .expect("hent_sak feilet");
    println!("{:#?}", resp);

    assert_eq!(resp.kildesystem.as_deref(), None);
    assert!(
        resp.saksnr
            .as_deref()
            .unwrap_or_default()
            .contains("2025/500961")
            || resp.saksid > 0,
        "Respons mangler forventet saksinfo"
    );
}

#[tokio::test]
#[ignore]
async fn hent_sak_e2e_henter_secrets_og_kaller_api_med_saksnummer_som_ikke_finnes() {
    unsafe {
        set_var(
            "BASE_URL_SIKRI",
            "https://services09.elementscloud.no/test/MattilsynetArkivApiTestCloud",
        );
    }
    let _ = env::var("APP_APPLICATION__PROJECT_ID")
        .expect("APP_APPLICATION__PROJECT_ID må være satt (GCP prosjekt-ID)");

    let project_id = env::var("APP_APPLICATION__PROJECT_ID")
        .expect("APP_APPLICATION__PROJECT_ID må være satt (GCP prosjekt-ID)");
    let base_url =
        env::var("BASE_URL_SIKRI").expect("BASE_URL_SIKRI må være satt til faktisk Sikri-base-URL");

    assert!(!project_id.is_empty());
    assert!(!base_url.is_empty());

    // Kall API med et saksnummer som ikke finnes og verifiser klassifiseringen.
    let feil = sikri_client::hent_sak("0000/00000", "MATS", false)
        .await
        .expect_err("Forventet 404 for not found");

    assert_eq!(feil.kode, "sikri_resource_not_found");
    assert!(
        !feil.er_recoverable(),
        "et ukjent saksnummer skal avvises terminalt, ikke retryes"
    );
}
