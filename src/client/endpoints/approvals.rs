//! URL builders for the Azure DevOps pipeline approvals API.

use super::Endpoints;

impl Endpoints {
    /// Constructs the URL for fetching pending pipeline approvals.
    pub fn approvals_pending(&self) -> String {
        let api_version = &self.api_version;
        format!(
            "{}/pipelines/approvals?state=pending&$expand=steps&api-version={api_version}",
            self.base_url
        )
    }

    /// Constructs the URL for submitting approval decisions.
    pub fn approvals_update(&self) -> String {
        let api_version = &self.api_version;
        format!(
            "{}/pipelines/approvals?api-version={api_version}",
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
}
