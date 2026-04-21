//! URL builders for the Azure DevOps REST API.

pub mod approvals;
pub mod boards;
pub mod builds;
pub mod definitions;
pub mod pull_requests;
pub mod retention;
pub mod web;

// --- Constants ---

/// Default Azure DevOps REST API version used when no override is supplied.
pub const DEFAULT_API_VERSION: &str = "7.1";
pub(crate) const TOP_BUILDS: u32 = 1000;
pub(crate) const TOP_DEFINITION_BUILDS: u32 = 20;
pub(crate) const TOP_DEFINITIONS: u32 = 1000;

// --- Helpers ---

/// Percent-encodes a string for use in a URL path segment.
///
/// Encodes control characters (0x00–0x1F, 0x7F) and the characters ` #?/&=+%`
/// as `%XX` hex pairs. All other UTF-8 bytes pass through unchanged.
fn encode_path_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        if b <= 0x1F
            || b == 0x7F
            || b == b' '
            || b == b'#'
            || b == b'?'
            || b == b'/'
            || b == b'&'
            || b == b'='
            || b == b'+'
            || b == b'%'
        {
            out.push('%');
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        } else {
            out.push(b as char);
        }
    }
    out
}

// --- Endpoints ---

/// Holds pre-computed base URLs for an Azure DevOps organization and project.
#[derive(Clone)]
pub struct Endpoints {
    pub(crate) base_url: String,
    pub(crate) web_base_url: String,
    pub(crate) api_version: std::sync::Arc<str>,
}

impl Endpoints {
    /// Creates a new set of endpoints for the given organization and project,
    /// using [`DEFAULT_API_VERSION`] for the REST API version.
    pub fn new(organization: &str, project: &str) -> Self {
        Self::new_with_api_version(organization, project, DEFAULT_API_VERSION)
    }

    /// Creates a new set of endpoints with an explicit API version override.
    pub fn new_with_api_version(organization: &str, project: &str, api_version: &str) -> Self {
        let org = encode_path_segment(organization);
        let proj = encode_path_segment(project);
        Self {
            base_url: format!("https://dev.azure.com/{org}/{proj}/_apis"),
            web_base_url: format!("https://dev.azure.com/{org}/{proj}"),
            api_version: std::sync::Arc::from(api_version),
        }
    }

    /// Overrides the API version used by URL builders.
    pub fn set_api_version(&mut self, api_version: &str) {
        self.api_version = std::sync::Arc::from(api_version);
    }

    /// Constructs endpoints rooted at an arbitrary base URL.
    ///
    /// Intended for integration tests that point the client at a mock HTTP
    /// server (e.g. `wiremock`). Hidden from the rendered docs.
    #[doc(hidden)]
    pub fn with_base_url(base_url: &str, organization: &str, project: &str) -> Self {
        let org = encode_path_segment(organization);
        let proj = encode_path_segment(project);
        let trimmed = base_url.trim_end_matches('/');
        Self {
            base_url: format!("{trimmed}/{org}/{proj}/_apis"),
            web_base_url: format!("{trimmed}/{org}/{proj}"),
            api_version: std::sync::Arc::from(DEFAULT_API_VERSION),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_encode_special_characters() {
        let ep = Endpoints::new("my org", "my project");
        assert!(ep.definitions().contains("my%20org"));
        assert!(ep.definitions().contains("my%20project"));
    }
}
