mod early_access;
mod event;
mod feature_flag;
mod identify;
mod queue;
mod view;

use self::queue::QueueWorkerHandle;

const POSTHOG_BATCH_LIMIT: usize = 20; // TODO(colemickens): revisit check posthog docs

#[derive(Debug, Clone)]
pub struct PosthogClient {
    pub(crate) api_key: String,
    pub(crate) queue_handle: QueueWorkerHandle,
}

impl PosthogClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        let worker = QueueWorkerHandle::start(base_url);
        let client = Self {
            api_key,
            queue_handle: worker,
        };
        client
    }
}
