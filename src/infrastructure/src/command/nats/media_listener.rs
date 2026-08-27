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
