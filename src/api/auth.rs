use anyhow::Result;
use azure_core::credentials::TokenCredential;
use azure_identity::DefaultAzureCredential;
use std::sync::Arc;

const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";

#[derive(Clone)]
pub struct AdoAuth {
    credential: Arc<dyn TokenCredential>,
}

impl AdoAuth {
    pub async fn new() -> Result<Self> {
        // DefaultAzureCredential::new() already returns Arc<DefaultAzureCredential>
        let credential: Arc<dyn TokenCredential> = DefaultAzureCredential::new()?;
        Ok(Self { credential })
    }

    pub async fn token(&self) -> Result<String> {
        let response = self
            .credential
            .get_token(&[&format!("{ADO_RESOURCE}/.default")])
            .await?;
        Ok(response.token.secret().to_string())
    }
}
