use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorConfig {
    pub relay_urls: Vec<String>,
    pub server_name: String,
    pub server_about: Option<String>,
    pub server_website: Option<String>,
    pub is_announced: bool,
    pub max_age_days: u32,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            relay_urls: vec!["wss://relay.contextvm.org".into()],
            server_name: "cordn-server".into(),
            server_about: None,
            server_website: None,
            is_announced: false,
            max_age_days: 30,
        }
    }
}

impl CoordinatorConfig {
    pub(crate) fn validate(&self) -> crate::Result<()> {
        if self.server_name.trim().is_empty() {
            return Err(crate::Error::InvalidConfig(
                "server name is required".into(),
            ));
        }
        if self.relay_urls.is_empty() {
            return Err(crate::Error::InvalidConfig(
                "at least one relay is required".into(),
            ));
        }
        for relay in &self.relay_urls {
            let url = url::Url::parse(relay)
                .map_err(|_| crate::Error::InvalidConfig(format!("invalid relay URL: {relay}")))?;
            if !matches!(url.scheme(), "ws" | "wss") || url.host_str().is_none() {
                return Err(crate::Error::InvalidConfig(format!(
                    "relay URL must use ws:// or wss://: {relay}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub config: CoordinatorConfig,
    pub private_key_hex: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub state: String,
    pub public_key: Option<String>,
    pub nprofile: Option<String>,
    pub last_error: Option<String>,
    pub battery_exempt: bool,
}

impl Default for ServiceStatus {
    fn default() -> Self {
        Self {
            state: "stopped".into(),
            public_key: None,
            nprofile: None,
            last_error: None,
            battery_exempt: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDiagnostics {
    pub logs: String,
    pub storage_bytes: u64,
    pub started_at: Option<u64>,
    pub key_packages: Option<u64>,
    pub welcomes: Option<u64>,
    pub join_requests: Option<u64>,
    pub group_messages: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigResponse {
    pub config: Option<CoordinatorConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_relay_urls() {
        assert!(CoordinatorConfig::default().validate().is_ok());

        let config = CoordinatorConfig {
            relay_urls: vec!["https://relay.example.com".into()],
            ..CoordinatorConfig::default()
        };
        assert!(config.validate().is_err());
    }
}
