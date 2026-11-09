# SKU-0025. Batchen kontrolleres samlet før mottak

Date: 2026-11-06
Last-reviewed: 2026-11-06
Tier: B
Status: Accepted
Crates: domain, application, infrastructure, skuffen-integration-tests

## Related

References: SKU-0008, SKU-0016, SKU-0009

## Context

En klient sendte en batch der malen ga samme UUID til `command_id`,
`client_reference` og `sak_key` i alle tre kommandoene. `OpprettSak` ble tatt
imot. Kommando 2 og 3 hadde samme `command_id`, ble tolket som gjentatte
innsendinger og hoppet stille over. Klienten fikk `Ok`.

Mottaket så bare på én kommando om gangen og bare på `command_id`. Det fantes
ingen kontroll av at identitetene i en batch henger sammen, og en
`client_reference` som allerede fantes, ga stille den eksisterende entiteten.

## Decision

R1 [5]: `arkiv.arkiver` kontrollerer hele batchen før noe skrives. Én feil avviser
  hele batchen, og svaret forklarer alle feilene, ikke bare den første.

R2 [5]: Kontrollen er en ren funksjon i domain over batchen og Skuffens egne fakta
  fra `command` og `entitet`. Arkivet spørres ikke i den synkrone kjeden.

R3 [5]: Hver kommando i en batch har sin egen `command_id`, og hver sak, journalpost
  og hvert dokument har sin egen `client_reference`.

R4 [5]: En `command_id` som allerede er mottatt med samme kommandotype, er en
  gjentatt innsending. Er den mottatt med en annen kommandotype, avvises batchen.

R5 [5]: En ny kommando kan ikke opprette en `client_reference` som finnes i Skuffen.
  En gjentatt innsending sjekkes ikke mot Skuffen, fordi det den oppretter
  finnes fra første innsending.

R6 [5]: En `sak_key` med `clientReference` må vise til en `client_reference` som
  opprettes tidligere i batchen eller finnes i Skuffen. `arkivId` kontrolleres
  bare i den asynkrone valideringen.

## Consequences

Klienten får feilen synkront, med kommandoens posisjon og type, i stedet for et
stille `Ok` eller en asynkron avvisning. Saksreferansen kontrolleres nå to
steder: her mot Skuffens egne fakta, og i valideringen mot arkivet.

Gjentatt innsending kjennes igjen på `command_id` og kommandotype alene. Samme
`command_id` og type med annet innhold godtas derfor som gjentakelse, og det nye
innholdet ignoreres. Innholdet sammenlignes ikke med hash.

Kontrollen leser før den skriver. To samtidige batcher kan begge passere med
samme nye `client_reference`; da vinner den første ved registrering, som før.
Feiler mottaket etter kontrollen, kan kommandoene før feilen være tatt imot;
samme batch sendt på nytt fullfører mottaket.
