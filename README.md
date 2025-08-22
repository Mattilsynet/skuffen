# rust-ci-conf
`rust-ci-conf` er et repo Team Landdyr bruker for å sette opp kontinuerlig integrasjon (CI) for Rust-prosjekter.
Vi eksperimenterer med å inkludere dette repo som et remote repo i våre prosjekter istedenfor å distributere det som en workflow.
Dette er for at prosjekter kan endre sine workflows lokalt uten å måtte endre her først.

## Funksjoner
- Automatisert bygging av Rust-prosjekter
- Kjøring av enhetstester
- Linting
- Audit check
- Deploy til Cloud Run miljøer

## Komme i gang
Gitt at du står i et nytt Rust prosjekt
```git
git remote add ci git@github.com:Mattilsynet/rust-ci-conf.git
git fetch ci
git merge --allow-unrelated ci/master
```

### Sett disse variablene i Github Settings: 
Under Environment, opprett environment(dev/prod). Disse må samsvare med de miljøene du finner i release.yaml i deploy matrix:
- PROJECT_ID
- PROJECT_NUMBER

Under Secret and variables -> Variables tab -> New repository variable
- ARTIFACT_REPO_ID
- SERVICE_NAME

I prosjektet ditt må man under src foldern endre .Dockerfile:
- YOUR_BINARY_NAME (må settes til package name i cargo.toml)

## Oppdater Cargo.toml
For at workflowen skal kunne bygge riktig må Cargo.toml inneholde en eksplisitt [[bin]]- eller [lib]-seksjon.
Eksempel for et binærprosjekt:
```toml
[package]
name = "mitt_prosjekt"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "mitt_prosjekt"
path = "src/main.rs"
```

Eksempel for et bibliotek:
```toml
[package]
name = "mitt_bibliotek"
version = "0.1.0"
edition = "2021"

[lib]
name = "mitt_bibliotek"
path = "src/lib.rs"
```

> [!IMPORTANT]
> `name` under `[[bin]]` må matche `package.name` i `Cargo.toml`.
> For `[lib]` anbefales det sterkt å bruke samme navn som `package.name` for å
> unngå forvirring.
> I begge tilfeller må verdien brukes i Dockerfile (`YOUR_BINARY_NAME`).

## Deploy til Cloud Run
I release.yaml er deploy definert slik:
```yaml
deploy:
  name: Deploy
  needs: upload
  strategy:
    matrix:
      env: ['dev']
  uses: ./.github/workflows/deploy.yaml
  with:
    env: ${{ matrix.env }}
```
Dette betyr at deploy kun går mot dev som standard.

> [!TIP]
> Hvis du ønsker å deploye til prod, må du legge til prod i listen:
> ```yaml
> matrix:
>  env: ['dev', 'prod']
> ```
