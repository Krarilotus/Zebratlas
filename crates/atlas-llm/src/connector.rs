//! Reverse provider. The transport is constructed only after account authentication;
//! never install one in the shared registry under a visitor-supplied account id.
use crate::registry::Connection;
use crate::request::ProviderOutput;
use crate::{
    ApiKey, Availability, CompletionRequest, ConnectionConfig, KeyPolicy, LlmError, Provider, ProviderKind, Result,
};
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait ConnectorTransport: Send + Sync + std::fmt::Debug {
    async fn available(&self) -> bool;
    async fn dispatch(&self, request: CompletionRequest) -> Result<ProviderOutput>;
}

#[derive(Debug)]
pub struct ConnectorProvider(Arc<dyn ConnectorTransport>);

impl ConnectorProvider {
    pub fn connection(
        name: String,
        default_model: Option<String>,
        transport: Arc<dyn ConnectorTransport>,
    ) -> Connection {
        Connection {
            config: ConnectionConfig {
                name,
                kind: Some(ProviderKind::Connector),
                label: Some("Your machine".into()),
                default_model,
                ..Default::default()
            },
            kind: ProviderKind::Connector,
            key_policy: KeyPolicy::None,
            provider: Arc::new(Self(transport)),
        }
    }
}

#[async_trait]
impl Provider for ConnectorProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Connector
    }
    async fn probe(&self) -> Availability {
        if self.0.available().await {
            Availability::ready("connected")
        } else {
            Availability::not_ready("connector-offline")
        }
    }
    async fn complete(&self, req: &CompletionRequest, model: &str, key: Option<&ApiKey>) -> Result<ProviderOutput> {
        if key.is_some() {
            return Err(LlmError::InvalidRequest("keys stay on the user's machine".into()));
        }
        let mut request = req.clone();
        request.model = Some(model.to_owned());
        self.0.dispatch(request).await
    }
}
