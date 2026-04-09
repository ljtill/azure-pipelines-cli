#[derive(Clone)]
pub struct Endpoints {
    base_url: String,
    web_url: String,
}

impl Endpoints {
    pub fn new(organization: &str, project: &str) -> Self {
        Self {
            base_url: format!("https://dev.azure.com/{}/{}/_apis", organization, project),
            web_url: format!("https://dev.azure.com/{}/{}", organization, project),
        }
    }

    pub fn definitions(&self) -> String {
        format!("{}/build/definitions?api-version=7.1", self.base_url)
    }

    pub fn builds_active(&self) -> String {
        format!(
            "{}/build/builds?statusFilter=inProgress&api-version=7.1&$top=100",
            self.base_url
        )
    }

    pub fn builds_recent(&self) -> String {
        format!(
            "{}/build/builds?api-version=7.1&$top=100&queryOrder=startTimeDescending",
            self.base_url
        )
    }

    pub fn builds_for_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/build/builds?definitions={}&api-version=7.1&$top=20&queryOrder=startTimeDescending",
            self.base_url, definition_id
        )
    }

    pub fn build(&self, build_id: u32) -> String {
        format!(
            "{}/build/builds/{}?api-version=7.1",
            self.base_url, build_id
        )
    }

    pub fn build_timeline(&self, build_id: u32) -> String {
        format!(
            "{}/build/builds/{}/timeline?api-version=7.1",
            self.base_url, build_id
        )
    }

    pub fn build_log(&self, build_id: u32, log_id: u32) -> String {
        format!(
            "{}/build/builds/{}/logs/{}?api-version=7.1",
            self.base_url, build_id, log_id
        )
    }

    pub fn build_stage(&self, build_id: u32, stage_ref_name: &str) -> String {
        format!(
            "{}/build/builds/{}/stages/{}?api-version=7.1-preview.1",
            self.base_url, build_id, stage_ref_name
        )
    }

    pub fn pipeline_runs(&self, pipeline_id: u32) -> String {
        format!(
            "{}/pipelines/{}/runs?api-version=7.1",
            self.base_url, pipeline_id
        )
    }

    // Web UI URLs for opening in browser

    pub fn web_build(&self, build_id: u32) -> String {
        format!("{}/_build/results?buildId={}", self.web_url, build_id)
    }

    pub fn web_definition(&self, definition_id: u32) -> String {
        format!("{}/_build?definitionId={}", self.web_url, definition_id)
    }

    pub fn web_active_builds(&self) -> String {
        format!("{}/_build?view=runs", self.web_url)
    }
}
