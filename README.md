## Skuffen
Skuffen er en tjeneste foran sikri sitt arkiv api. 
- Gjor retries ved recoverable errors
-  
 
## Eksempel
 nats request sak.hent '{
  "key": {
     "type": "arkivId",
     "value": "2025/513910"
  },
  "inkluderJournalposter": true
}
'
