//! URL builders for the Azure DevOps REST API.

pub mod approvals;
pub mod boards;
pub mod builds;
pub mod definitions;
pub mod pull_requests;
pub mod retention;
pub mod web;

// --- Constants ---

const API_VERSION: &str = "7.1";
pub(crate) const TOP_BUILDS: u32 = 100;
pub(crate) const TOP_DEFINITION_BUILDS: u32 = 20;

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
}

impl Endpoints {
    /// Creates a new set of endpoints for the given organization and project.
    pub fn new(organization: &str, project: &str) -> Self {
        let org = encode_path_segment(organization);
        let proj = encode_path_segment(project);
        Self {
            base_url: format!("https://dev.azure.com/{org}/{proj}/_apis"),
            web_base_url: format!("https://dev.azure.com/{org}/{proj}"),
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
