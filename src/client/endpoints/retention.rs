//! URL builders for the Azure DevOps build retention API.

use super::{API_VERSION, Endpoints};

impl Endpoints {
    /// Constructs the URL for fetching retention leases for a build definition.
    pub fn retention_leases_for_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/build/retention/leases?definitionId={definition_id}&api-version={API_VERSION}",
            self.base_url
        )
    }

    /// Constructs the URL for deleting retention leases by their IDs.
    pub fn retention_leases_delete(&self, ids: &[u32]) -> String {
        let ids_str: Vec<String> = ids.iter().map(std::string::ToString::to_string).collect();
        format!(
            "{}/build/retention/leases?ids={}&api-version={API_VERSION}",
            self.base_url,
            ids_str.join(",")
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
}
