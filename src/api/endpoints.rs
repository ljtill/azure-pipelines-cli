const API_VERSION: &str = "7.1";
const TOP_BUILDS: u32 = 100;
const TOP_DEFINITION_BUILDS: u32 = 20;

/// Percent-encode a string for use in a URL path segment.
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
            // Write uppercase hex pair
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        } else {
            out.push(b as char);
        }
    }
    out
}

#[derive(Clone)]
pub struct Endpoints {
    base_url: String,
    web_base_url: String,
}

impl Endpoints {
    pub fn new(organization: &str, project: &str) -> Self {
        let org = encode_path_segment(organization);
        let proj = encode_path_segment(project);
        Self {
            base_url: format!("https://dev.azure.com/{}/{}/_apis", org, proj),
            web_base_url: format!("https://dev.azure.com/{}/{}", org, proj),
        }
    }

    pub fn web_build(&self, build_id: u32) -> String {
        format!("{}/_build/results?buildId={}", self.web_base_url, build_id)
    }

    pub fn web_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/_build?definitionId={}",
            self.web_base_url, definition_id
        )
    }

    pub fn definitions(&self) -> String {
        format!(
            "{}/build/definitions?api-version={API_VERSION}&includeLatestBuilds=true",
            self.base_url
        )
    }

    pub fn builds_recent(&self) -> String {
        format!(
            "{}/build/builds?api-version={API_VERSION}&$top={TOP_BUILDS}&queryOrder=queueTimeDescending",
            self.base_url
        )
    }

    pub fn builds_for_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/build/builds?definitions={definition_id}&api-version={API_VERSION}&$top={TOP_DEFINITION_BUILDS}&queryOrder=queueTimeDescending",
            self.base_url
        )
    }

    pub fn build(&self, build_id: u32) -> String {
        format!(
            "{}/build/builds/{build_id}?api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn build_timeline(&self, build_id: u32) -> String {
        format!(
            "{}/build/builds/{build_id}/timeline?api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn build_log(&self, build_id: u32, log_id: u32) -> String {
        format!(
            "{}/build/builds/{build_id}/logs/{log_id}?api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn build_stage(&self, build_id: u32, stage_ref_name: &str) -> String {
        format!(
            "{}/build/builds/{build_id}/stages/{stage_ref_name}?api-version={API_VERSION}-preview.1",
            self.base_url
        )
    }

    pub fn pipeline_runs(&self, pipeline_id: u32) -> String {
        format!(
            "{}/pipelines/{pipeline_id}/runs?api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn approvals_pending(&self) -> String {
        format!(
            "{}/pipelines/approvals?state=pending&$expand=steps&api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn approvals_update(&self) -> String {
        format!(
            "{}/pipelines/approvals?api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn retention_leases_for_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/build/retention/leases?definitionId={definition_id}&api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn retention_leases_delete(&self, ids: &[u32]) -> String {
        let ids_str: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        format!(
            "{}/build/retention/leases?ids={}&api-version={API_VERSION}",
            self.base_url,
            ids_str.join(",")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep() -> Endpoints {
        Endpoints::new("myorg", "myproj")
    }

    const BASE: &str = "https://dev.azure.com/myorg/myproj/_apis";

    #[test]
    fn definitions_url() {
        assert_eq!(
            ep().definitions(),
            format!("{BASE}/build/definitions?api-version=7.1&includeLatestBuilds=true")
        );
    }

    #[test]
    fn builds_recent_url() {
        assert_eq!(
            ep().builds_recent(),
            format!("{BASE}/build/builds?api-version=7.1&$top=100&queryOrder=queueTimeDescending")
        );
    }

    #[test]
    fn builds_for_definition_url() {
        assert_eq!(
            ep().builds_for_definition(42),
            format!(
                "{BASE}/build/builds?definitions=42&api-version=7.1&$top=20&queryOrder=queueTimeDescending"
            )
        );
    }

    #[test]
    fn build_url() {
        assert_eq!(
            ep().build(123),
            format!("{BASE}/build/builds/123?api-version=7.1")
        );
    }

    #[test]
    fn build_timeline_url() {
        assert_eq!(
            ep().build_timeline(123),
            format!("{BASE}/build/builds/123/timeline?api-version=7.1")
        );
    }

    #[test]
    fn build_log_url() {
        assert_eq!(
            ep().build_log(123, 7),
            format!("{BASE}/build/builds/123/logs/7?api-version=7.1")
        );
    }

    #[test]
    fn build_stage_url() {
        assert_eq!(
            ep().build_stage(123, "__default"),
            format!("{BASE}/build/builds/123/stages/__default?api-version=7.1-preview.1")
        );
    }

    #[test]
    fn pipeline_runs_url() {
        assert_eq!(
            ep().pipeline_runs(42),
            format!("{BASE}/pipelines/42/runs?api-version=7.1")
        );
    }

    #[test]
    fn approvals_pending_url() {
        assert_eq!(
            ep().approvals_pending(),
            format!("{BASE}/pipelines/approvals?state=pending&$expand=steps&api-version=7.1")
        );
    }

    #[test]
    fn approvals_update_url() {
        assert_eq!(
            ep().approvals_update(),
            format!("{BASE}/pipelines/approvals?api-version=7.1")
        );
    }

    #[test]
    fn retention_leases_for_definition_url() {
        assert_eq!(
            ep().retention_leases_for_definition(42),
            format!("{BASE}/build/retention/leases?definitionId=42&api-version=7.1")
        );
    }

    #[test]
    fn retention_leases_delete_url() {
        assert_eq!(
            ep().retention_leases_delete(&[1, 2, 3]),
            format!("{BASE}/build/retention/leases?ids=1,2,3&api-version=7.1")
        );
    }

    #[test]
    fn retention_leases_delete_single_url() {
        assert_eq!(
            ep().retention_leases_delete(&[42]),
            format!("{BASE}/build/retention/leases?ids=42&api-version=7.1")
        );
    }

    const WEB_BASE: &str = "https://dev.azure.com/myorg/myproj";

    #[test]
    fn web_build_url() {
        assert_eq!(
            ep().web_build(42),
            format!("{WEB_BASE}/_build/results?buildId=42")
        );
    }

    #[test]
    fn web_definition_url() {
        assert_eq!(
            ep().web_definition(10),
            format!("{WEB_BASE}/_build?definitionId=10")
        );
    }

    #[test]
    fn endpoints_encode_special_characters() {
        let ep = Endpoints::new("my org", "my project");
        assert!(ep.definitions().contains("my%20org"));
        assert!(ep.definitions().contains("my%20project"));
    }
}
