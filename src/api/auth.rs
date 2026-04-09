use std::sync::Arc;

use anyhow::Result;
use azure_core::{credentials::TokenCredential, time::OffsetDateTime};
use azure_identity::DeveloperToolsCredential;
use tokio::sync::RwLock;

const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";

/// Margin before actual expiry to trigger a refresh, avoiding edge-case failures.
const EXPIRY_MARGIN: std::time::Duration = std::time::Duration::from_secs(120);

struct CachedToken {
    secret: String,
    expires_on: std::time::Instant,
}

#[derive(Clone)]
pub struct AdoAuth {
    credential: Arc<dyn TokenCredential>,
    cache: Arc<RwLock<Option<CachedToken>>>,
}

impl AdoAuth {
    pub async fn new() -> Result<Self> {
        let credential: Arc<dyn TokenCredential> = DeveloperToolsCredential::new(None)?;
        Ok(Self {
            credential,
            cache: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn token(&self) -> Result<String> {
        // Fast path: check cached token under read lock
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.expires_on > std::time::Instant::now()
            {
                return Ok(cached.secret.clone());
            }
        }

        // Slow path: refresh token under write lock
        let mut cache = self.cache.write().await;
        // Double-check: another task may have refreshed while we waited for the lock
        if let Some(cached) = cache.as_ref()
            && cached.expires_on > std::time::Instant::now()
        {
            return Ok(cached.secret.clone());
        }

        let response = self
            .credential
            .get_token(&[&format!("{ADO_RESOURCE}/.default")], None)
            .await?;

        let secret = response.token.secret().to_string();
        let secs_until = response
            .expires_on
            .unix_timestamp()
            .saturating_sub(OffsetDateTime::now_utc().unix_timestamp());
        let expires_on = if secs_until > EXPIRY_MARGIN.as_secs() as i64 {
            std::time::Instant::now()
                + std::time::Duration::from_secs(
                    (secs_until as u64).saturating_sub(EXPIRY_MARGIN.as_secs()),
                )
        } else {
            std::time::Instant::now() // Already near expiry → will refresh next call
        };

        *cache = Some(CachedToken {
            secret: secret.clone(),
            expires_on,
        });

        Ok(secret)
    }
}
