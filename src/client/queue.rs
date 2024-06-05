use reqwest::{Client, Method};
use serde_json::{json, Value};
use tokio::sync::{
    mpsc::{self, unbounded_channel},
    oneshot::Sender,
};

use crate::{client::POSTHOG_BATCH_LIMIT, error::PosthogError};

#[derive(Debug)]
pub enum PosthogRequest {
    /// Capture an event.
    ///
    /// Endpoint: /capture
    /// Method: POST
    CaptureEvent { body: Value },

    /// Capture multiple events in a single request.
    ///
    /// Endpoint: /batch
    /// Method: POST
    CaptureBatch { body: Value },

    /// Evaluate feature flags.
    ///
    /// Endpoint: /decide?v=3
    /// Method: POST
    EvaluateFeatureFlags { body: Value },

    /// Get early access features.
    ///
    /// Endpoint: /api/early_access_features
    /// Method: GET
    GetEarlyAccessFeatures { api_key: String },

    /// Any other request.
    ///
    /// Endpoint: Any
    /// Method: Any
    Other {
        method: Method,
        endpoint: String,
        json: Value,
    },
}

impl Default for PosthogRequest {
    fn default() -> Self {
        Self::Other {
            method: Method::GET,
            endpoint: String::new(),
            json: Value::Null,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct QueuedRequest {
    pub(crate) request: PosthogRequest,

    pub(crate) response_tx: Option<Sender<Result<Value, PosthogError>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct QueueWorkerHandle {
    pub(crate) tx: mpsc::UnboundedSender<QueuedRequest>,
    // extra default inner client used for immediate requests
    pub(crate) inner_client: QueueWorkerInner,
}

#[derive(Clone, Debug)]
pub(crate) struct QueueWorkerInner {
    pub base_url: String,
    pub client: Client,
}

impl QueueWorkerHandle {
    pub(crate) fn start(base_url: String) -> QueueWorkerHandle {
        let (tx, mut rx) = unbounded_channel::<QueuedRequest>();

        let worker = QueueWorkerInner {
            base_url,
            client: Client::new(),
        };
        let immediate_worker = worker.clone();

        tokio::spawn(async move {
            loop {
                let mut buffer: Vec<QueuedRequest> = Vec::new();
                let x = rx.recv_many(&mut buffer, POSTHOG_BATCH_LIMIT).await;
                if x == 0 {
                    return;
                }

                let mut batch_capture = Vec::new();

                for request in buffer.into_iter() {
                    match request.request {
                        PosthogRequest::CaptureEvent { body } if request.response_tx.is_none() => {
                            batch_capture.push(body);
                        }

                        _ => {
                            worker.handle_request(request).await;
                        }
                    }
                }

                if !batch_capture.is_empty() {
                    // The API key is added by the client to each event, so we can just take it from the first event.
                    let api_key = batch_capture[0]["api_key"].as_str().unwrap();

                    let body = json!({
                       "api_key": api_key,
                       "batch": batch_capture,
                    });

                    let request = QueuedRequest {
                        request: PosthogRequest::CaptureBatch { body },
                        response_tx: None,
                    };

                    worker.handle_request(request).await;
                }
            }
        });

        QueueWorkerHandle {
            tx,
            inner_client: immediate_worker,
        }
    }

    pub fn dispatch_request(&self, request: QueuedRequest) {
        let worker = self.inner_client.clone();
        tokio::spawn(async move {
            worker.handle_request(request).await;
        });
    }
}
impl QueueWorkerInner {
    async fn handle_request(&self, request: QueuedRequest) {
        let (method, endpoint, body) = match request.request {
            PosthogRequest::CaptureEvent { body } => (Method::POST, "capture".to_string(), body),

            PosthogRequest::CaptureBatch { body } => (Method::POST, "batch".to_string(), body),

            PosthogRequest::EvaluateFeatureFlags { body } => {
                (Method::POST, "decide?v=3".to_string(), body)
            }

            PosthogRequest::GetEarlyAccessFeatures { api_key } => (
                Method::GET,
                format!("api/early_access_features?api_key={}", api_key),
                Value::Null,
            ),

            PosthogRequest::Other {
                method,
                endpoint,
                json,
            } => (method, endpoint, json),
        };

        let response = self.send_request(method, endpoint, body).await;

        if let Some(response_tx) = request.response_tx {
            response_tx.send(response).ok();
        }
    }

    async fn send_request(
        &self,
        method: Method,
        endpoint: impl Into<String>,
        json: Value,
    ) -> Result<Value, PosthogError> {
        let response = self
            .client
            .request(method, &format!("{}/{}", self.base_url, endpoint.into()))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&json)
            .send()
            .await
            .map_err(PosthogError::HttpError)?;

        if response.status().is_success() {
            response.json().await.map_err(PosthogError::HttpError)
        } else {
            Err(PosthogError::HttpError(
                response.error_for_status().unwrap_err(),
            ))
        }
    }
}
