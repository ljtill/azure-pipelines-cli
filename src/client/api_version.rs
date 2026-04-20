//! Runtime override for the Azure DevOps REST API version used by [`AdoClient`].
//!
//! Kept in a separate file so the core HTTP module stays narrowly focused on
//! transport concerns. The default API version is
//! [`crate::client::endpoints::DEFAULT_API_VERSION`]; callers can override it
//! via [`AdoClient::set_api_version`] before issuing any requests.

use super::http::AdoClient;

impl AdoClient {
    /// Overrides the REST API version used when building endpoint URLs.
    pub fn set_api_version(&mut self, api_version: &str) {
        self.endpoints.set_api_version(api_version);
    }
}
