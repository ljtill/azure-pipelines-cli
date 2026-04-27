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
/// Encodes every byte except RFC 3986 unreserved characters as `%XX` hex pairs.
fn encode_path_segment(input: &str) -> String {
    percent_encode(input)
}

/// Percent-encodes a string for use in a URL query value.
///
/// Encodes every byte except RFC 3986 unreserved characters as `%XX` hex pairs.
fn encode_query_value(input: &str) -> String {
    percent_encode(input)
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        if is_unreserved_url_byte(b) {
            out.push(char::from(b));
        } else {
            out.push('%');
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            out.push(char::from(HEX[(b >> 4) as usize]));
            out.push(char::from(HEX[(b & 0x0F) as usize]));
        }
    }
    out
}

const fn is_unreserved_url_byte(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
    )
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
    use crate::client::endpoints::pull_requests::PullRequestListRequest;

    #[test]
    fn endpoints_encode_special_characters() {
        let ep = Endpoints::new("my org", "my project");
        assert!(ep.definitions().contains("my%20org"));
        assert!(ep.definitions().contains("my%20project"));
    }

    #[test]
    fn path_segments_encode_reserved_and_unicode_bytes() {
        assert_eq!(
            encode_path_segment("name /?#%&=+\"'é"),
            "name%20%2F%3F%23%25%26%3D%2B%22%27%C3%A9"
        );
    }

    #[test]
    fn path_and_query_encoders_match_ascii_percent_encoding_rules() {
        for byte in 0u8..=127 {
            let input = char::from(byte).to_string();
            let expected = if is_unreserved_ascii(byte) {
                input.clone()
            } else {
                format!("%{byte:02X}")
            };

            assert_eq!(
                encode_path_segment(&input),
                expected,
                "path byte {byte:#04X}"
            );
            assert_eq!(
                encode_query_value(&input),
                expected,
                "query byte {byte:#04X}"
            );
        }
    }

    #[test]
    fn path_and_query_encoders_round_trip_edge_case_values() {
        let samples = [
            "",
            "simple",
            "space value",
            "slash/path",
            "question?and&equals=value",
            "fragment#percent%plus+",
            "\"quotes\" and 'apostrophe'",
            "back\\slash",
            "line\nbreak",
            "tab\tvalue",
            "emoji 🦀",
            "café",
            "漢字",
            "\u{0}nul",
        ];

        for raw in samples {
            for (kind, encoded) in [
                ("path", encode_path_segment(raw)),
                ("query", encode_query_value(raw)),
            ] {
                assert_valid_percent_encoding(&encoded, kind, raw);
                assert_eq!(
                    percent_decode(&encoded),
                    raw,
                    "{kind} encoding should round trip for {raw:?}"
                );
            }
        }
    }

    #[test]
    fn endpoints_construct_encoded_roots_from_sampled_path_segments() {
        let cases = [
            ("simple", "project", "simple", "project"),
            (
                "space org",
                "space project",
                "space%20org",
                "space%20project",
            ),
            ("org/one", "project?two", "org%2Fone", "project%3Ftwo"),
            ("org#frag", "proj%value", "org%23frag", "proj%25value"),
            ("café", "emoji 🦀", "caf%C3%A9", "emoji%20%F0%9F%A6%80"),
        ];

        for (organization, project, encoded_org, encoded_project) in cases {
            let endpoints = Endpoints::new(organization, project);
            assert_eq!(
                endpoints.definitions(),
                format!(
                    "https://dev.azure.com/{encoded_org}/{encoded_project}/_apis/build/definitions?api-version=7.1&includeLatestBuilds=true&$top=1000"
                )
            );
            assert_eq!(
                endpoints.project_teams(),
                format!(
                    "https://dev.azure.com/{encoded_org}/_apis/projects/{encoded_project}/teams?api-version=7.1"
                )
            );
            assert_eq!(
                endpoints.work_items_batch(),
                format!(
                    "https://dev.azure.com/{encoded_org}/_apis/wit/workitemsbatch?api-version=7.1"
                )
            );
        }
    }

    #[test]
    fn endpoint_path_parameters_escape_sampled_delimiters() {
        let endpoints = Endpoints::new("org", "project");
        let cases = [
            ("simple", "simple"),
            ("space value", "space%20value"),
            ("slash/path", "slash%2Fpath"),
            ("query?and&equals=value", "query%3Fand%26equals%3Dvalue"),
            ("fragment#percent%plus+", "fragment%23percent%25plus%2B"),
            ("quote\"'", "quote%22%27"),
            ("café 🦀", "caf%C3%A9%20%F0%9F%A6%80"),
        ];

        for (raw, encoded) in cases {
            assert_eq!(
                endpoints.build_stage(9, raw),
                format!(
                    "https://dev.azure.com/org/project/_apis/build/builds/9/stages/{encoded}?api-version=7.1-preview.1"
                )
            );
            assert_eq!(
                endpoints.backlogs(raw),
                format!(
                    "https://dev.azure.com/org/project/{encoded}/_apis/work/backlogs?api-version=7.1"
                )
            );
            assert_eq!(
                endpoints.backlog_level_work_items(raw, raw),
                format!(
                    "https://dev.azure.com/org/project/{encoded}/_apis/work/backlogs/{encoded}/workItems?api-version=7.1"
                )
            );
            assert_eq!(
                endpoints.pull_request(raw, 2),
                format!(
                    "https://dev.azure.com/org/project/_apis/git/repositories/{encoded}/pullrequests/2?api-version=7.1"
                )
            );
            assert_eq!(
                endpoints.pull_request_threads(raw, 2),
                format!(
                    "https://dev.azure.com/org/project/_apis/git/repositories/{encoded}/pullrequests/2/threads?api-version=7.1"
                )
            );
            assert_eq!(
                endpoints.web_pull_request(raw, 2),
                format!("https://dev.azure.com/org/project/_git/{encoded}/pullrequest/2")
            );
        }
    }

    #[test]
    fn pull_request_query_values_escape_sampled_delimiters() {
        let cases = [
            ("simple", "simple"),
            ("space value", "space%20value"),
            ("slash/path", "slash%2Fpath"),
            ("query?and&equals=value", "query%3Fand%26equals%3Dvalue"),
            ("fragment#percent%plus+", "fragment%23percent%25plus%2B"),
            ("quote\"'", "quote%22%27"),
            ("café 🦀", "caf%C3%A9%20%F0%9F%A6%80"),
        ];

        for (raw, encoded) in cases {
            let mut endpoints = Endpoints::new("org", "project");
            endpoints.set_api_version(raw);

            assert_eq!(
                endpoints.pull_requests_for_project(
                    PullRequestListRequest::active()
                        .with_creator_id(raw)
                        .with_reviewer_id(raw),
                ),
                format!(
                    "https://dev.azure.com/org/project/_apis/git/pullrequests?api-version={encoded}&searchCriteria.status=active&$top=100&searchCriteria.creatorId={encoded}&searchCriteria.reviewerId={encoded}"
                )
            );
        }
    }

    fn is_unreserved_ascii(byte: u8) -> bool {
        matches!(
            byte,
            b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'-'
                | b'.'
                | b'_'
                | b'~'
        )
    }

    fn assert_valid_percent_encoding(encoded: &str, kind: &str, raw: &str) {
        let bytes = encoded.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if is_unreserved_ascii(bytes[i]) {
                i += 1;
                continue;
            }

            assert_eq!(
                bytes[i], b'%',
                "{kind} encoding for {raw:?} left byte {:#04X} unescaped in {encoded:?}",
                bytes[i]
            );
            assert!(
                i + 2 < bytes.len(),
                "{kind} encoding for {raw:?} ended with incomplete percent escape in {encoded:?}"
            );
            assert!(
                matches!(bytes[i + 1], b'0'..=b'9' | b'A'..=b'F')
                    && matches!(bytes[i + 2], b'0'..=b'9' | b'A'..=b'F'),
                "{kind} encoding for {raw:?} used a non-uppercase percent escape in {encoded:?}"
            );
            i += 3;
        }
    }

    fn percent_decode(encoded: &str) -> String {
        let bytes = encoded.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'%' {
                decoded.push((hex_value(bytes[i + 1]) << 4) | hex_value(bytes[i + 2]));
                i += 3;
            } else {
                decoded.push(bytes[i]);
                i += 1;
            }
        }

        String::from_utf8(decoded).expect("percent-decoded samples are valid UTF-8")
    }

    fn hex_value(byte: u8) -> u8 {
        match byte {
            b'0'..=b'9' => byte - b'0',
            b'A'..=b'F' => byte - b'A' + 10,
            other => panic!("invalid uppercase hex byte {other:#04X}"),
        }
    }
}
