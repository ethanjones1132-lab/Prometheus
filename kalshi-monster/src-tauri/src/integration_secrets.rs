//! Lightweight HTTP probes for optional data API keys (Brave, FRED).

use serde::Serialize;

use crate::chat::web_context;
use crate::config::AppConfig;

#[derive(Debug, Clone, Serialize)]
pub struct SecretServiceProbe {
    pub service: String,
    pub configured: bool,
    pub ok: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrationSecretsHealth {
    pub brave: SecretServiceProbe,
    pub fred: SecretServiceProbe,
}

fn resolve_fred_key(config: &AppConfig) -> Option<String> {
    let from_cfg = config.fred_api_key.trim();
    if !from_cfg.is_empty() {
        return Some(from_cfg.to_string());
    }
    std::env::var("FRED_API_KEY")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn probe_brave(key: &str) -> SecretServiceProbe {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return SecretServiceProbe {
                service: "brave".into(),
                configured: true,
                ok: false,
                detail: Some(format!("HTTP client: {e}")),
            };
        }
    };
    let url = "https://api.search.brave.com/res/v1/web/search?q=kalshi&count=1";
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", key)
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => SecretServiceProbe {
            service: "brave".into(),
            configured: true,
            ok: true,
            detail: Some("Search API reachable".into()),
        },
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            SecretServiceProbe {
                service: "brave".into(),
                configured: true,
                ok: false,
                detail: Some(format!(
                    "HTTP {status}: {}",
                    body.chars().take(120).collect::<String>()
                )),
            }
        }
        Err(e) => SecretServiceProbe {
            service: "brave".into(),
            configured: true,
            ok: false,
            detail: Some(format!("Request failed: {e}")),
        },
    }
}

async fn probe_fred(key: &str) -> SecretServiceProbe {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return SecretServiceProbe {
                service: "fred".into(),
                configured: true,
                ok: false,
                detail: Some(format!("HTTP client: {e}")),
            };
        }
    };
    let url = format!(
        "https://api.stlouisfed.org/fred/series/observations?series_id=GNPCA&api_key={}&file_type=json&limit=1",
        urlencoding::encode(key)
    );
    let resp = client.get(&url).send().await;
    match resp {
        Ok(r) if r.status().is_success() => SecretServiceProbe {
            service: "fred".into(),
            configured: true,
            ok: true,
            detail: Some("FRED observations OK".into()),
        },
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            SecretServiceProbe {
                service: "fred".into(),
                configured: true,
                ok: false,
                detail: Some(format!(
                    "HTTP {status}: {}",
                    body.chars().take(120).collect::<String>()
                )),
            }
        }
        Err(e) => SecretServiceProbe {
            service: "fred".into(),
            configured: true,
            ok: false,
            detail: Some(format!("Request failed: {e}")),
        },
    }
}

fn not_configured(service: &str, hint: &str) -> SecretServiceProbe {
    SecretServiceProbe {
        service: service.into(),
        configured: false,
        ok: false,
        detail: Some(hint.into()),
    }
}

/// Probe Brave + FRED keys used by Analyst web grounding and macro agent.
pub async fn check_integration_secrets_health(config: &AppConfig) -> IntegrationSecretsHealth {
    let brave_key = web_context::resolve_brave_api_key(Some(&config.brave_api_key));
    let brave = match brave_key {
        Some(k) => probe_brave(&k).await,
        None => not_configured(
            "brave",
            "No Brave key (Settings or BRAVE_API_KEY) — web falls back to DuckDuckGo",
        ),
    };

    let fred_key = resolve_fred_key(config);
    let fred = match fred_key {
        Some(k) => probe_fred(&k).await,
        None => not_configured(
            "fred",
            "No FRED key (Settings or FRED_API_KEY) — macro agent stays null",
        ),
    };

    IntegrationSecretsHealth { brave, fred }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_configured_probe_shape() {
        let p = not_configured("fred", "hint");
        assert!(!p.configured);
        assert!(!p.ok);
        assert_eq!(p.service, "fred");
    }
}