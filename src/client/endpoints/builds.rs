//! URL builders for the Azure DevOps builds API.

use super::{API_VERSION, Endpoints, TOP_BUILDS, TOP_DEFINITION_BUILDS};

impl Endpoints {
    /// Constructs the URL for fetching recent builds across all definitions.
    pub fn builds_recent(&self) -> String {
        format!(
            "{}/build/builds?api-version={API_VERSION}&$top={TOP_BUILDS}&queryOrder=queueTimeDescending",
            self.base_url
        )
    }

    /// Constructs the URL for fetching recent builds for a specific definition.
    pub fn builds_for_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/build/builds?definitions={definition_id}&api-version={API_VERSION}&$top={TOP_DEFINITION_BUILDS}&queryOrder=queueTimeDescending",
            self.base_url
        )
    }

    /// Constructs the URL for fetching builds for a definition with a custom page size.
    pub fn builds_for_definition_with_top(&self, definition_id: u32, top: u32) -> String {
        format!(
            "{}/build/builds?definitions={definition_id}&api-version={API_VERSION}&$top={top}&queryOrder=queueTimeDescending",
            self.base_url
        )
    }

    /// Constructs the URL for fetching a single build by ID.
    pub fn build(&self, build_id: u32) -> String {
        format!(
            "{}/build/builds/{build_id}?api-version={API_VERSION}",
            self.base_url
        )
    }

    /// Constructs the URL for fetching a build's timeline records.
    pub fn build_timeline(&self, build_id: u32) -> String {
        format!(
            "{}/build/builds/{build_id}/timeline?api-version={API_VERSION}",
            self.base_url
        )
    }

    /// Constructs the URL for fetching a specific build log by log ID.
    pub fn build_log(&self, build_id: u32, log_id: u32) -> String {
        format!(
            "{}/build/builds/{build_id}/logs/{log_id}?api-version={API_VERSION}",
            self.base_url
        )
    }

    /// Constructs the URL for updating a build stage by reference name.
    pub fn build_stage(&self, build_id: u32, stage_ref_name: &str) -> String {
        let stage_ref_name = super::encode_path_segment(stage_ref_name);
        format!(
            "{}/build/builds/{build_id}/stages/{stage_ref_name}?api-version={API_VERSION}-preview.1",
            self.base_url
        )
    }

    /// Constructs the URL for creating a new run of a pipeline.
    pub fn pipeline_runs(&self, pipeline_id: u32) -> String {
        format!(
            "{}/pipelines/{pipeline_id}/runs?api-version={API_VERSION}",
            self.base_url
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::client::endpoints::Endpoints;

    fn ep() -> Endpoints {
        Endpoints::new("myorg", "myproj")
    }

    const BASE: &str = "https://dev.azure.com/myorg/myproj/_apis";

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
    fn builds_for_definition_with_top_url() {
        assert_eq!(
            ep().builds_for_definition_with_top(42, 50),
            format!(
                "{BASE}/build/builds?definitions=42&api-version=7.1&$top=50&queryOrder=queueTimeDescending"
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
    fn build_stage_url_encodes_reserved_characters() {
        assert_eq!(
            ep().build_stage(123, "stage name/%?&=+#"),
            format!(
                "{BASE}/build/builds/123/stages/stage%20name%2F%25%3F%26%3D%2B%23?api-version=7.1-preview.1"
            )
        );
    }

    #[test]
    fn pipeline_runs_url() {
        assert_eq!(
            ep().pipeline_runs(42),
            format!("{BASE}/pipelines/42/runs?api-version=7.1")
        );
    }
}
