const API_VERSION: &str = "7.1";
const TOP_BUILDS: u32 = 100;
const TOP_DEFINITION_BUILDS: u32 = 20;

#[derive(Clone)]
pub struct Endpoints {
    base_url: String,
}

impl Endpoints {
    pub fn new(organization: &str, project: &str) -> Self {
        Self {
            base_url: format!("https://dev.azure.com/{}/{}/_apis", organization, project),
        }
    }

    pub fn definitions(&self) -> String {
        format!(
            "{}/build/definitions?api-version={API_VERSION}",
            self.base_url
        )
    }

    pub fn builds_active(&self) -> String {
        format!(
            "{}/build/builds?statusFilter=inProgress&api-version={API_VERSION}&$top={TOP_BUILDS}",
            self.base_url
        )
    }

    pub fn builds_recent(&self) -> String {
        format!(
            "{}/build/builds?api-version={API_VERSION}&$top={TOP_BUILDS}&queryOrder=startTimeDescending",
            self.base_url
        )
    }

    pub fn builds_for_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/build/builds?definitions={definition_id}&api-version={API_VERSION}&$top={TOP_DEFINITION_BUILDS}&queryOrder=startTimeDescending",
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
}
