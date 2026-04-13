use std::time::Duration;

use anyhow::Context;
use async_nats::jetstream::{
    self, consumer, consumer::PullConsumer, context::ConsumerInfoErrorKind, stream,
};

const COMMAND_STREAM_MAX_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 180);

pub fn command_inbox_stream_config(num_replicas: usize) -> stream::Config {
    stream::Config {
        name: "arkiv_command_inbox".to_string(),
        subjects: vec!["arkiv.command.inbox.>".to_string()],
        max_age: COMMAND_STREAM_MAX_AGE,
        num_replicas,
        ..Default::default()
    }
}

pub fn command_ready_stream_config(num_replicas: usize) -> stream::Config {
    stream::Config {
        name: "arkiv_command_ready".to_string(),
        subjects: vec!["arkiv.command.ready.>".to_string()],
        max_age: COMMAND_STREAM_MAX_AGE,
        num_replicas,
        ..Default::default()
    }
}

pub fn command_done_stream_config(num_replicas: usize) -> stream::Config {
    stream::Config {
        name: "arkiv_command_done".to_string(),
        subjects: vec!["arkiv.command.done.>".to_string()],
        max_age: COMMAND_STREAM_MAX_AGE,
        num_replicas,
        ..Default::default()
    }
}

pub fn status_stream_config(num_replicas: usize) -> stream::Config {
    stream::Config {
        name: "arkiv_status".to_string(),
        subjects: vec!["arkiv.status.*".to_string()],
        max_age: COMMAND_STREAM_MAX_AGE,
        num_replicas,
        ..Default::default()
    }
}

pub fn validator_consumer_config(num_replicas: usize) -> consumer::pull::Config {
    consumer::pull::Config {
        durable_name: Some("validator".to_string()),
        ack_policy: consumer::AckPolicy::Explicit,
        num_replicas,
        ..Default::default()
    }
}

pub fn executor_consumer_config(num_replicas: usize) -> consumer::pull::Config {
    consumer::pull::Config {
        durable_name: Some("executor".to_string()),
        ack_policy: consumer::AckPolicy::Explicit,
        num_replicas,
        ..Default::default()
    }
}

pub fn media_object_store_config(num_replicas: usize) -> jetstream::object_store::Config {
    jetstream::object_store::Config {
        bucket: "arkiv_media".to_string(),
        num_replicas,
        ..Default::default()
    }
}

pub async fn ensure_stream(
    context: &jetstream::Context,
    config: stream::Config,
) -> anyhow::Result<stream::Stream> {
    let name = config.name.clone();
    context
        .create_or_update_stream(config)
        .await
        .with_context(|| format!("failed to create or update stream {name}"))?;

    context
        .get_stream(name.clone())
        .await
        .with_context(|| format!("failed to load stream {name}"))
}

pub async fn ensure_pull_consumer(
    stream: &stream::Stream,
    name: &str,
    config: consumer::pull::Config,
) -> anyhow::Result<PullConsumer> {
    match stream.consumer_info(name).await {
        Ok(_) => stream.update_consumer(config).await.with_context(|| {
            format!(
                "failed to update consumer {name} on stream {}",
                stream.cached_info().config.name
            )
        }),
        Err(err) if matches!(err.kind(), ConsumerInfoErrorKind::NotFound) => {
            stream.create_consumer(config).await.with_context(|| {
                format!(
                    "failed to create consumer {name} on stream {}",
                    stream.cached_info().config.name
                )
            })
        }
        Err(err) => Err(anyhow::Error::new(err).context(format!(
            "failed to inspect consumer {name} on stream {}",
            stream.cached_info().config.name
        ))),
    }
}

pub async fn ensure_media_object_store(
    context: &jetstream::Context,
    num_replicas: usize,
) -> anyhow::Result<jetstream::object_store::ObjectStore> {
    let config = media_object_store_config(num_replicas);
    ensure_stream(context, media_object_store_stream_config(&config)).await?;

    context
        .get_object_store(config.bucket.clone())
        .await
        .with_context(|| format!("failed to load object store {}", config.bucket))
}

fn media_object_store_stream_config(config: &jetstream::object_store::Config) -> stream::Config {
    let bucket = config.bucket.clone();

    stream::Config {
        name: format!("OBJ_{bucket}"),
        description: config.description.clone(),
        subjects: vec![format!("$O.{bucket}.C.>"), format!("$O.{bucket}.M.>")],
        max_age: config.max_age,
        max_bytes: config.max_bytes,
        storage: config.storage,
        num_replicas: config.num_replicas,
        discard: stream::DiscardPolicy::New,
        allow_rollup: true,
        allow_direct: true,
        placement: config.placement.clone(),
        ..Default::default()
    }
}
