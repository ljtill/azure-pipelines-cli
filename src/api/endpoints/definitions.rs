use super::{API_VERSION, Endpoints};

impl Endpoints {
    pub fn definitions(&self) -> String {
        format!(
            "{}/build/definitions?api-version={API_VERSION}&includeLatestBuilds=true",
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
    fn definitions_url() {
        assert_eq!(
            ep().definitions(),
            format!("{BASE}/build/definitions?api-version=7.1&includeLatestBuilds=true")
        );
    }
}
