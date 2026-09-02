# SKU-0021. Bakgrunnstasks er supervisert, ellers dør prosessen

Date: 2026-09-02
Last-reviewed: 2026-09-02
Tier: C
Status: Accepted
Crates: skuffen, infrastructure

## Related

References: SKU-0016, SKU-0020

## Context

`src/lib.rs` spawner ti navngitte tasks. To er infrastruktur (`signal_handler`,
`health_check`); åtte gjør arbeid: `query_listener`, `ready_replier`,
`media_listener`, `command_listener`, `validation_listener`, `admin_listener`,
`execution_listener` og `execution_worker`.

`TaskSupervisor` finnes med restartbudsjett og et `stable_run_window` som
nullstiller telleren etter stabil drift. Bare fire av de åtte bruker den, og bare
to får nedstengingssignalet. `query_listener`, `ready_replier` og
`media_listener` har ingen supervisor.

Orkestreringen skiller `Critical` fra `Degraded`. En `Degraded` task som stopper
gir én `error!`-linje og så går løkka videre. Tasken respawnes ikke. Det gjelder
også når den returnerer `Err`:

```rust
Err(err) => match outcome.criticality {
    TaskCriticality::Critical => { tasks.abort_all(); return Err(...); }
    TaskCriticality::Degraded => { tracing::error!(...); }   // bare logg
}
```

Å legge supervisorer på tasksene løser derfor ingenting alene. Supervisoren
returnerer `Err` når budsjettet er tømt, og orkestreringen svelger det.

`QueryListener::run` gjør i tillegg noe som gjør supervisjon virkningsløs:

```rust
let (sak, journalpost, bruker) = tokio::join!(a.run(), b.run(), c.run());
```

`NatsReplier::run` returnerer `Ok(())` når subscription-strømmen tar slutt.
`tokio::join!` venter på alle tre, så én avsluttet subscription blokkerer for
alltid uten at noen får vite det. `AdminListener` bruker `try_join!` nettopp av
denne grunnen, og sier det i en kommentar.

Helsesjekken er `Router::new().route("/", get(|| async { StatusCode::OK }))`.
Den sjekker ikke database, ikke NATS, ikke om noen task lever. Cloud Run har
verken liveness- eller startup-probe konfigurert i `modules/skuffen`, så
defaulten gjelder: en TCP-probe som lykkes i det porten bindes — og porten
bindes før migrasjonene kjører.

Summen er en revisjon som svarer 200 mens ingenting behandles, og som Cloud Run
aldri restarter fordi ingenting forteller den at noe er galt.

## Decision

R1 [7]: Hver langlevd task kjører under `TaskSupervisor` med **endelig**
  restartbudsjett og nedstengingstoken. Verken bar `tokio::spawn` eller
  ubegrenset `background()` for arbeid som skal leve like lenge som prosessen.

R2 [7]: Budsjettet er fem forsøk, og det er rullende. Stabil drift nullstiller
  telleren, så en task som feiler sjelden over lang tid dør ikke av akkumulerte
  restarter.

R3 [8]: Tømt restartbudsjett returnerer `Err` og avslutter prosessen med
  exitkode ulik null. Kritikalitet gater ikke dette. En task som ikke kom seg
  opp av fem restarter er ikke degradert, den er død.

R4 [7]: Kritikalitet sier bare hva som skjer når en task avslutter rent utenfor
  nedstenging. Kritisk er inntaksstien — kommandolytteren, medialytteren og
  helseserveren. Alt annet er degradert.

R5 [7]: Readiness rapporterer aggregert tilstand: migrasjoner ferdige, NATS
  tilkoblet, alle tasks oppe. Liveness beviser kun at runtimen svarer, og skal
  ikke avhenge av eksterne systemer.

R6 [8]: Porten bindes først av alt, før NATS og migrasjoner, så startup-proben
  kan lykkes. Readiness er usann til migrasjonene er ferdige. Probene
  konfigureres i Cloud Run; endepunkter uten prober er dekorasjon.

R7 [7]: En task som samler flere subscriptions avsluttes når den første av dem
  avsluttes. `try_join!`, aldri `join!`. En strøm som tar slutt er en feil som
  skal nå supervisoren, ikke en gren som venter for alltid.

## Consequences

En instans som mister en task blir enten frisk igjen eller dør. Mellomtilstanden
der prosessen lever og lyver om det, forsvinner. Det er hele poenget: Cloud Run
kan bare reparere en død container, aldri en halvdød.

R3 gjør `TaskCriticality` nesten overflødig. Med `with_shutdown` returnerer en
superviserte task `Ok(())` bare ved nedstenging, så den ene grenen kritikalitet
fortsatt styrer, inntreffer i praksis kun for `health_check` og `signal_handler`,
som ikke er superviserte. Enumet beholdes fordi det er billig og fordi det gjør
intensjonen lesbar, ikke fordi det bærer mye vekt.

Helsetilstanden får én kilde. Samme 1/0-flagg leses av `/health/ready` i dag og
av en metrikk senere, uten omskriving. `TaskSupervisor` teller restarter i minne
av samme grunn — tallet finnes allerede, det er bare ikke eksponert.

Readiness styrer ikke trafikk her. NATS går utenom Cloud Runs load balancer, så
readiness er et signal til mennesker og dashboards. Liveness og prosess-exit er
den faktiske håndhevelsen, og det skal ikke forveksles.

Vi aksepterer at en Postgres-utetid gir usann readiness uten å drepe containeren.
Restart reparerer ikke en nede database, og en restartloop gjør recovery
tregere.
