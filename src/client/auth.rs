//! Bearer token authentication via Azure CLI credentials.
//!
//! Uses `DeveloperToolsCredential` (Azure CLI / Azure Developer CLI chain) to
//! obtain OAuth bearer tokens for the Azure DevOps resource. Tokens are cached
//! in memory and refreshed automatically before they expire.

use std::sync::Arc;

use anyhow::Result;
use azure_core::{credentials::TokenCredential, time::OffsetDateTime};
use azure_identity::DeveloperToolsCredential;
use tokio::sync::RwLock;

use crate::shared::SecretString;

const ADO_RESOURCE: &str = "499b84ac-1321-427f-aa17-267ca6975798";

/// Specifies the margin before actual expiry to trigger a refresh, avoiding edge-case failures.
const EXPIRY_MARGIN: std::time::Duration = std::time::Duration::from_mins(2);

/// Holds a cached bearer token alongside its computed expiry instant.
struct CachedToken {
    secret: SecretString,
    expires_on: std::time::Instant,
}

/// Provides Azure DevOps bearer token authentication with in-memory caching.
#[derive(Clone)]
pub struct AdoAuth {
    credential: Arc<dyn TokenCredential>,
    cache: Arc<RwLock<Option<CachedToken>>>,
}

impl AdoAuth {
    /// Creates a new authenticator backed by `DeveloperToolsCredential`.
    pub fn new() -> Result<Self> {
        let credential: Arc<dyn TokenCredential> = DeveloperToolsCredential::new(None)?;
        Ok(Self {
            credential,
            cache: Arc::new(RwLock::new(None)),
        })
    }

    /// Returns a valid bearer token, refreshing from the credential chain if the cache is stale.
    pub async fn token(&self) -> Result<SecretString> {
        // Fast path: check cached token under read lock.
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.expires_on > std::time::Instant::now()
            {
                tracing::trace!("auth token cache hit");
                return Ok(cached.secret.clone());
            }
        }

        // Slow path: refresh token under write lock.
        let mut cache = self.cache.write().await;
        // Double-check: another task may have refreshed while we waited for the lock.
        if let Some(cached) = cache.as_ref()
            && cached.expires_on > std::time::Instant::now()
        {
            tracing::trace!("auth token cache hit (after write lock)");
            return Ok(cached.secret.clone());
        }

        tracing::debug!("refreshing auth token");
        let response = self
            .credential
            .get_token(&[&format!("{ADO_RESOURCE}/.default")], None)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "authentication failed");
                anyhow::anyhow!(
                    "Authentication failed — ensure you are logged in with `az login` or `azd auth login`.\n\nUnderlying error: {e}"
                )
            })?;

        let secret = SecretString::from(response.token.secret().to_string());
        let secs_until = response
            .expires_on
            .unix_timestamp()
            .saturating_sub(OffsetDateTime::now_utc().unix_timestamp());
        let expires_on = if secs_until > EXPIRY_MARGIN.as_secs() as i64 {
            let effective_secs = (secs_until as u64).saturating_sub(EXPIRY_MARGIN.as_secs());
            tracing::debug!(expires_in_secs = effective_secs, "auth token refreshed");
            std::time::Instant::now() + std::time::Duration::from_secs(effective_secs)
        } else {
            // Already near expiry; will refresh on the next call.
            tracing::debug!("auth token near expiry, will refresh next call");
            std::time::Instant::now()
        };

        let returned = secret.clone();
        *cache = Some(CachedToken { secret, expires_on });

        Ok(returned)
    }

    /// Creates an authenticator with a pre-seeded bearer token.
    ///
    /// Intended for integration tests so the credential chain is never
    /// exercised. The cached token is given a far-future expiry so every
    /// `token()` call is served from the cache. Hidden from the rendered docs.
    #[doc(hidden)]
    pub fn with_static_token(token: &str) -> Result<Self> {
        let credential: Arc<dyn TokenCredential> = DeveloperToolsCredential::new(None)?;
        let cached = CachedToken {
            secret: SecretString::from(token.to_string()),
            expires_on: std::time::Instant::now() + std::time::Duration::from_hours(1),
        };
        Ok(Self {
            credential,
            cache: Arc::new(RwLock::new(Some(cached))),
        })
    }
}
