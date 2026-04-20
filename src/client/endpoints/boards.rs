//! URL builders for Azure Boards and work item tracking APIs.

use super::{Endpoints, encode_path_segment};

impl Endpoints {
    /// Constructs the URL for listing project teams.
    pub fn project_teams(&self) -> String {
        let api_version = &self.api_version;
        let (org_base, project) = self
            .web_base_url
            .rsplit_once('/')
            .unwrap_or((self.web_base_url.as_str(), ""));
        format!("{org_base}/_apis/projects/{project}/teams?api-version={api_version}")
    }

    /// Constructs the URL for listing backlog levels for a team.
    pub fn backlogs(&self, team: &str) -> String {
        let api_version = &self.api_version;
        let team = encode_path_segment(team);
        format!(
            "{}/{team}/_apis/work/backlogs?api-version={api_version}",
            self.web_base_url
        )
    }

    /// Constructs the URL for listing work item IDs in a specific backlog level.
    pub fn backlog_level_work_items(&self, team: &str, backlog_id: &str) -> String {
        let api_version = &self.api_version;
        let team = encode_path_segment(team);
        let backlog_id = encode_path_segment(backlog_id);
        format!(
            "{}/{team}/_apis/work/backlogs/{backlog_id}/workItems?api-version={api_version}",
            self.web_base_url
        )
    }

    /// Constructs the URL for listing project work item type categories.
    pub fn work_item_type_categories(&self) -> String {
        let api_version = &self.api_version;
        format!(
            "{}/wit/workitemtypecategories?api-version={api_version}",
            self.base_url
        )
    }

    /// Constructs the URL for executing a WIQL query.
    pub fn wiql(&self) -> String {
        let api_version = &self.api_version;
        format!("{}/wit/wiql?api-version={api_version}", self.base_url)
    }

    /// Constructs the URL for fetching work items in batch.
    pub fn work_items_batch(&self) -> String {
        let api_version = &self.api_version;
        let (org_base, _) = self
            .web_base_url
            .rsplit_once('/')
            .unwrap_or((self.web_base_url.as_str(), ""));
        format!("{org_base}/_apis/wit/workitemsbatch?api-version={api_version}")
    }

    /// Constructs the URL for listing comments on a work item.
    /// Uses the 7.1-preview.3 comments endpoint.
    pub fn work_item_comments(&self, work_item_id: u32) -> String {
        format!(
            "{}/wit/workItems/{work_item_id}/comments?api-version=7.1-preview.3",
            self.base_url
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::client::endpoints::Endpoints;

    fn ep() -> Endpoints {
        Endpoints::new("my org", "my project")
    }

    const BASE: &str = "https://dev.azure.com/my%20org/my%20project/_apis";
    const WEB_BASE: &str = "https://dev.azure.com/my%20org/my%20project";

    #[test]
    fn project_teams_url() {
        assert_eq!(
            ep().project_teams(),
            "https://dev.azure.com/my%20org/_apis/projects/my%20project/teams?api-version=7.1"
        );
    }

    #[test]
    fn backlogs_url_encodes_team() {
        assert_eq!(
            ep().backlogs("My Team/One"),
            format!("{WEB_BASE}/My%20Team%2FOne/_apis/work/backlogs?api-version=7.1")
        );
    }

    #[test]
    fn backlog_level_work_items_url_encodes_inputs() {
        assert_eq!(
            ep().backlog_level_work_items("My Team", "Microsoft.RequirementCategory"),
            format!(
                "{WEB_BASE}/My%20Team/_apis/work/backlogs/Microsoft.RequirementCategory/workItems?api-version=7.1"
            )
        );
    }

    #[test]
    fn work_item_type_categories_url() {
        assert_eq!(
            ep().work_item_type_categories(),
            format!("{BASE}/wit/workitemtypecategories?api-version=7.1")
        );
    }

    #[test]
    fn wiql_url() {
        assert_eq!(ep().wiql(), format!("{BASE}/wit/wiql?api-version=7.1"));
    }

    #[test]
    fn work_items_batch_url() {
        assert_eq!(
            ep().work_items_batch(),
            "https://dev.azure.com/my%20org/_apis/wit/workitemsbatch?api-version=7.1"
        );
    }

    #[test]
    fn work_item_comments_url_uses_preview_api_version() {
        assert_eq!(
            ep().work_item_comments(42),
            format!("{BASE}/wit/workItems/42/comments?api-version=7.1-preview.3")
        );
    }
}
