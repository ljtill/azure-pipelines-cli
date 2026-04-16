//! URL builders for Azure DevOps web portal links.

use super::Endpoints;

impl Endpoints {
    /// Constructs the web portal URL for viewing a build result.
    pub fn web_build(&self, build_id: u32) -> String {
        format!("{}/_build/results?buildId={}", self.web_base_url, build_id)
    }

    /// Constructs the web portal URL for viewing a build definition.
    pub fn web_definition(&self, definition_id: u32) -> String {
        format!(
            "{}/_build?definitionId={}",
            self.web_base_url, definition_id
        )
    }

    /// Constructs the web portal URL for viewing a work item.
    pub fn web_work_item(&self, work_item_id: u32) -> String {
        format!("{}/_workitems/edit/{work_item_id}", self.web_base_url)
    }
}

#[cfg(test)]
mod tests {
    use crate::client::endpoints::Endpoints;

    fn ep() -> Endpoints {
        Endpoints::new("myorg", "myproj")
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
    fn web_work_item_url() {
        assert_eq!(
            ep().web_work_item(555),
            format!("{WEB_BASE}/_workitems/edit/555")
        );
    }
}
