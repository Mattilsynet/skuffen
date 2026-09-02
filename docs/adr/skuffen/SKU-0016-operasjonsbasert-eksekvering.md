# SKU-0016. Operasjonsbasert eksekvering

Date: 2026-08-18
Last-reviewed: 2026-09-02
Tier: A
Status: Accepted
Crates: skuffen, domain, application, infrastructure, sikri_client, skuffen-integration-tests

## Related

Root: SKU-0016 | Supersedes: SKU-0001, SKU-0007, SKU-0010

## Context

Execution v2 er kommandobasert: én rad per kommando, og neste arkivkall beregnes
på nytt ved hvert forsøk. Operasjonene er dermed flyktige. Konsekvensen er at
ingen enkeltoperasjon kan navngis, retryes eller rapporteres, at det ikke finnes
noen rad å journalføre intensjonen i før arkivskrivet, og at ett feilende vedlegg
stopper hele kommandoen i strid med best-effort-prinsippet.

## Decision

R1 [4]: En operasjon er ett arkivkall med egen id, egen status, egen retry og egen
statuslinje utad. Kommandoen er mottaksjournal og idempotency-nøkkel; operasjonen
er eksekveringsenheten.

R2 [4]: Dekomponering fra kommando til operasjoner skjer én gang, ved innlesing av
den validerte kommandoen. Operasjonslisten er en ren funksjon av command payload.
Det finnes ingen re-planlegging.

R3 [4]: Avhengigheter mellom operasjoner utledes fra facts, ikke fra lagrede
kanter. Ingen `depends_on`, ingen `sekvensnr`. `AvsluttSak` er eneste unntak: den
krever at alle andre operasjoner på saken er terminalt ok.

R4 [5]: Skriveoperasjoner commiter `klar → sendt` før arkivkallet, og `sendt → ok`
med arkivsvar og faktaoppdatering i én transaksjon etterpå. Dette er stedet
at-most-once-grensen registreres.

R5 [5]: En operasjon funnet i `sendt` ved recovery har ukjent utfall og går til
`krever_avklaring`. Ingen automatisk rekonsiliering mot arkivet; et menneske
rydder opp.

R6 [5]: Recoverable feil retryes for alltid med eksponentiell backoff opp til én
gang per døgn. Ingen maks antall forsøk. Kun irrecoverable feil gir terminal
`feilet`.

R7 [5]: Ett permanent feilet søsken stopper ikke andre operasjoner. Best effort:
alt som lovlig kan utføres, utføres, selv om kommandoen som helhet aldri blir ok.

R8 [5]: Kommandoen er terminal ok når alle operasjoner er terminalt ok, og
terminal feilet når minst én er terminalt feilet. Terminal feil publiseres
umiddelbart fordi foldet er monotont.

R9 [5]: `terminal: true` betyr at utfallet er avgjort, ikke at flere eventer er
utelukket. Operasjonseventer kan fortsette etterpå fordi søsken kjører videre
best effort.

R10 [5]: Journalposter opprettes aldri direkte i `J`. `Journalfør`,
`SettEkspedert`, `KlargjørForEkspedering` og `Avskriv` er eksplisitte operasjoner,
og Skuffen setter aldri `J` på utgående.

R11 [5]: `entitet` er master for `skuffen_id` og eneste sted arkiv-id-er bor.
Idempotency-nøkkelen for en kommando er `kommando.dispatchet_at`, ikke radens
eksistens.

R12 [5]: Dekomponering materialiserer attributter inn i state-tabellene, i én
transaksjon. Executor leser aldri command payload og rører aldri wire-typer.

## Consequences

Operasjonsstatus blir spørrbar og adresserbar utad. At-most-once koster en ekstra
commit per arkivskriv og innfører `krever_avklaring` som en tilstand drift må
rydde manuelt. Kommandostatus finnes ikke som kolonne — den er et fold over
`operasjon`. `command_execution`, `id_mapping` og `tilstand_historikk` utgår.

Hendelsesdrevet wake-up ble opprinnelig erstattet av et periodisk evalueringspass.
Passet viste seg å være et andre beslutningssted som skrev terminale utfall uten å
publisere dem, og SKU-0020 fjerner det til fordel for én forfallsklokke og ett
beslutningssted. R1–R12 står uendret.
