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

Sett disse variablene i Github Settings, under Environment opprett environment(dev/prod), disse må samsvare med de miljøene du finner i release.yaml i deploy matrix :
PROJECT_ID
PROJECT_NUMBER

Under Secret and variables -> Variables tab -> New repository variable
ARTIFACT_REPO_ID
SERVICE_NAME

I prosjektet ditt må man under src foldern endre .Dockerfile
YOUR_BINARY_NAME må settes til package name i cargo.toml

release.yaml kan endres om man skal legge til andre miljøer enn dev/prod
