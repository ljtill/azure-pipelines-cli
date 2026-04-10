use super::{API_VERSION, Endpoints};

impl Endpoints {
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

#[cfg(test)]
mod tests {
    use crate::api::endpoints::Endpoints;

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
