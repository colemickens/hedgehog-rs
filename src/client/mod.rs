mod builder;
mod early_access;
mod event;
mod feature_flag;
mod identify;
mod queue;
mod view;

pub use builder::PosthogClientBuilder;
use tokio::sync::mpsc;

use self::queue::{QueueWorker, QueuedRequest};

#[derive(Debug, Clone)]
pub struct PosthogClient {
    pub(crate) api_key: String,

    // NOTE(colemickens): move this to PosthogClient so that owner can
    // drop this. If its in QueueWorker, it gets cloned into the spawned thread
    // and prevents clean shutdown.
    pub(crate) worker_tx: mpsc::UnboundedSender<QueuedRequest>,
}

impl PosthogClient {
    pub fn builder() -> PosthogClientBuilder {
        PosthogClientBuilder::new()
    }

    pub(crate) fn new(base_url: String, api_key: String) -> Self {
        let (_, worker_tx) = QueueWorker::start(base_url);
        let client = Self {
            api_key,
            worker_tx,
        };
        client
    }
}
