use std::sync::Arc;
use std::time::Duration;

use lib_nats::chunked_upload::{
    ChunkedUploadServer, ChunkedUploadServerConfig, ChunkedUploadServerError,
};
use tokio_util::sync::CancellationToken;

use crate::command::media::ObjectStoreMediaStore;
use crate::nats::client::NatsClient;

const MEDIA_UPLOAD_BASE_SUBJECT: &str = "arkiv.arkiver.media";
const MEDIA_UPLOAD_BEGIN_QUEUE: &str = "skuffen-media-processor";
/// Taket for samtidig reservert opplastingsminne. Lavere enn lib-nats sin
/// default på 500 MB fordi containeren har 1 GiB, og ett dokument gjennom
/// Sikri-gatewayen koster ~370 MB i tillegg (100 MB rå, base64, JSON-body).
const MEDIA_MAX_RESERVED_BYTES: u64 = 256 * 1024 * 1024;

pub struct MediaListener {
    server: ChunkedUploadServer,
}

impl MediaListener {
    pub fn new(
        client: NatsClient,
        store: Arc<ObjectStoreMediaStore>,
    ) -> Result<Self, ChunkedUploadServerError> {
        let config = ChunkedUploadServerConfig {
            base_subject: MEDIA_UPLOAD_BASE_SUBJECT.to_string(),
            begin_queue: MEDIA_UPLOAD_BEGIN_QUEUE.to_string(),
            shutdown_grace: Duration::from_secs(5),
            max_reserved_bytes: MEDIA_MAX_RESERVED_BYTES,
            ..ChunkedUploadServerConfig::default()
        };
        let server = ChunkedUploadServer::new(client.inner().clone(), config, store)?;
        Ok(Self { server })
    }

    #[tracing::instrument(skip_all, name = "nats.media_listener")]
    pub async fn run(self, shutdown: CancellationToken) -> Result<(), ChunkedUploadServerError> {
        self.server.run(shutdown.cancelled_owned()).await
    }
}
