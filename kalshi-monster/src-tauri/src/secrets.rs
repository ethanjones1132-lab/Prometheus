//! OS credential store integration for sensitive configuration values.
//!
//! Secrets (API keys, passwords, bot tokens) are stored in the platform keychain
//! instead of `config.json`. Each secret is addressed by a fixed service name
//! (`com.kalshimonster.desktop`) and a per-secret account key.

use crate::config::AppConfig;
use serde::{Deserialize, Serialize};

const SERVICE_NAME: &str = "com.kalshimonster.desktop";

/// Named secrets stored in the OS credential store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretKey {
    OpenrouterApiKey,
    OpencodeApiKey,
    OpenweathermapApiKey,
    ApiSportsKey,
    BraveApiKey,
    KalshiPassword,
    DiscordWebhookUrl,
    TelegramBotToken,
}

impl SecretKey {
    /// Keyring account name for this secret.
    pub fn account(&self) -> &'static str {
        match self {
            SecretKey::OpenrouterApiKey => "openrouter_api_key",
            SecretKey::OpencodeApiKey => "opencode_api_key",
            SecretKey::OpenweathermapApiKey => "openweathermap_api_key",
            SecretKey::ApiSportsKey => "api_sports_key",
            SecretKey::BraveApiKey => "brave_api_key",
            SecretKey::KalshiPassword => "kalshi_password",
            SecretKey::DiscordWebhookUrl => "discord_webhook_url",
            SecretKey::TelegramBotToken => "telegram_bot_token",
        }
    }

    /// Parse from the kebab-case / snake-case account name used by IPC.
    pub fn from_account(s: &str) -> Option<Self> {
        match s {
            "openrouter_api_key" => Some(SecretKey::OpenrouterApiKey),
            "opencode_api_key" => Some(SecretKey::OpencodeApiKey),
            "openweathermap_api_key" => Some(SecretKey::OpenweathermapApiKey),
            "api_sports_key" => Some(SecretKey::ApiSportsKey),
            "brave_api_key" => Some(SecretKey::BraveApiKey),
            "kalshi_password" => Some(SecretKey::KalshiPassword),
            "discord_webhook_url" => Some(SecretKey::DiscordWebhookUrl),
            "telegram_bot_token" => Some(SecretKey::TelegramBotToken),
            _ => None,
        }
    }

    fn entry(&self) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(SERVICE_NAME, self.account())
    }
}

/// In-memory bundle of secret values. These values are never serialized to
/// `config.json`; they are loaded from the OS credential store on demand.
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct AppSecrets {
    pub openrouter_api_key: String,
    pub opencode_api_key: String,
    pub openweathermap_api_key: String,
    pub api_sports_key: String,
    pub brave_api_key: String,
    pub kalshi_password: String,
    pub discord_webhook_url: String,
    pub telegram_bot_token: String,
}

impl AppSecrets {
    /// Load all secrets from the OS credential store. Missing entries return
    /// empty strings, which is the conventional "not configured" state.
    pub fn load() -> Result<Self, String> {
        Ok(Self {
            openrouter_api_key: get_secret(SecretKey::OpenrouterApiKey),
            opencode_api_key: get_secret(SecretKey::OpencodeApiKey),
            openweathermap_api_key: get_secret(SecretKey::OpenweathermapApiKey),
            api_sports_key: get_secret(SecretKey::ApiSportsKey),
            brave_api_key: get_secret(SecretKey::BraveApiKey),
            kalshi_password: get_secret(SecretKey::KalshiPassword),
            discord_webhook_url: get_secret(SecretKey::DiscordWebhookUrl),
            telegram_bot_token: get_secret(SecretKey::TelegramBotToken),
        })
    }

    /// Apply these secrets to an `AppConfig` so existing callers that take an
    /// `&AppConfig` can continue to read secret fields.
    pub fn apply_to(&self, config: &mut AppConfig) {
        config.openrouter_api_key = self.openrouter_api_key.clone();
        config.opencode_api_key = self.opencode_api_key.clone();
        config.openweathermap_api_key = self.openweathermap_api_key.clone();
        config.api_sports_key = self.api_sports_key.clone();
        config.brave_api_key = self.brave_api_key.clone();
        config.kalshi_password = self.kalshi_password.clone();
        config.discord_webhook_url = self.discord_webhook_url.clone();
        config.telegram_bot_token = self.telegram_bot_token.clone();
    }

    /// Extract secret values from a legacy `AppConfig` that was loaded from a
    /// plaintext `config.json`.
    pub fn from_config(config: &AppConfig) -> Self {
        Self {
            openrouter_api_key: config.openrouter_api_key.clone(),
            opencode_api_key: config.opencode_api_key.clone(),
            openweathermap_api_key: config.openweathermap_api_key.clone(),
            api_sports_key: config.api_sports_key.clone(),
            brave_api_key: config.brave_api_key.clone(),
            kalshi_password: config.kalshi_password.clone(),
            discord_webhook_url: config.discord_webhook_url.clone(),
            telegram_bot_token: config.telegram_bot_token.clone(),
        }
    }

    /// Returns true if at least one secret field is non-empty.
    pub fn has_any(&self) -> bool {
        !self.openrouter_api_key.is_empty()
            || !self.opencode_api_key.is_empty()
            || !self.openweathermap_api_key.is_empty()
            || !self.api_sports_key.is_empty()
            || !self.brave_api_key.is_empty()
            || !self.kalshi_password.is_empty()
            || !self.discord_webhook_url.is_empty()
            || !self.telegram_bot_token.is_empty()
    }
}

fn get_secret(key: SecretKey) -> String {
    match key.entry() {
        Ok(entry) => match entry.get_password() {
            Ok(value) => value,
            Err(keyring::Error::NoEntry) => String::new(),
            Err(e) => {
                tracing::warn!("failed to read secret {} from keyring: {}", key.account(), e);
                String::new()
            }
        },
        Err(e) => {
            tracing::warn!("failed to open keyring entry for {}: {}", key.account(), e);
            String::new()
        }
    }
}

/// Store a single secret in the OS credential store.
pub fn save_secret(key: SecretKey, value: &str) -> Result<(), String> {
    let entry = key.entry().map_err(|e| format!("keyring entry: {}", e))?;
    if value.is_empty() {
        // An empty value means "remove / not configured".
        match entry.delete_credential() {
            Ok(()) => {}
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("delete secret {}: {}", key.account(), e)),
        }
    } else {
        entry
            .set_password(value)
            .map_err(|e| format!("set secret {}: {}", key.account(), e))?;
    }
    Ok(())
}

/// Delete a single secret from the OS credential store.
pub fn delete_secret(key: SecretKey) -> Result<(), String> {
    save_secret(key, "")
}

/// Migrate secrets from a plaintext `AppConfig` into the OS credential store.
/// Returns the migrated secrets. Any individual secret that fails to store is
/// logged but does not abort the whole migration.
pub fn migrate_plaintext_secrets(config: &AppConfig) -> Result<AppSecrets, String> {
    let secrets = AppSecrets::from_config(config);

    let pairs: &[(SecretKey, &str)] = &[
        (SecretKey::OpenrouterApiKey, &secrets.openrouter_api_key),
        (SecretKey::OpencodeApiKey, &secrets.opencode_api_key),
        (SecretKey::OpenweathermapApiKey, &secrets.openweathermap_api_key),
        (SecretKey::ApiSportsKey, &secrets.api_sports_key),
        (SecretKey::BraveApiKey, &secrets.brave_api_key),
        (SecretKey::KalshiPassword, &secrets.kalshi_password),
        (SecretKey::DiscordWebhookUrl, &secrets.discord_webhook_url),
        (SecretKey::TelegramBotToken, &secrets.telegram_bot_token),
    ];

    for (key, value) in pairs {
        if !value.is_empty() {
            if let Err(e) = save_secret(*key, value) {
                tracing::error!("secret migration failed for {}: {}", key.account(), e);
            }
        }
    }

    Ok(secrets)
}

/// Returns true if the OS credential store is functional enough to use.
/// A quick smoke test creates a temporary entry, reads it back, and deletes it.
pub fn keyring_available() -> bool {
    let test_key = SecretKey::OpenrouterApiKey;
    let probe = format!("__kalshi_monster_probe_{}", uuid::Uuid::new_v4());
    let entry = match test_key.entry() {
        Ok(e) => e,
        Err(_) => return false,
    };
    if entry.set_password(&probe).is_err() {
        return false;
    }
    let ok = entry.get_password().map(|v| v == probe).unwrap_or(false);
    let _ = entry.delete_credential();
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_key_account_round_trip() {
        let keys = [
            SecretKey::OpenrouterApiKey,
            SecretKey::OpencodeApiKey,
            SecretKey::OpenweathermapApiKey,
            SecretKey::ApiSportsKey,
            SecretKey::BraveApiKey,
            SecretKey::KalshiPassword,
            SecretKey::DiscordWebhookUrl,
            SecretKey::TelegramBotToken,
        ];
        for key in keys {
            assert_eq!(SecretKey::from_account(key.account()), Some(key));
        }
    }

    #[test]
    fn app_secrets_from_config_copies_values() {
        let mut config = AppConfig::default();
        config.openrouter_api_key = "sk-or-test".into();
        config.kalshi_password = "hunter2".into();
        let secrets = AppSecrets::from_config(&config);
        assert_eq!(secrets.openrouter_api_key, "sk-or-test");
        assert_eq!(secrets.kalshi_password, "hunter2");
    }

    #[test]
    fn app_secrets_apply_to_config() {
        let mut secrets = AppSecrets::default();
        secrets.openrouter_api_key = "sk-or-test".into();
        secrets.telegram_bot_token = "123:abc".into();
        let mut config = AppConfig::default();
        secrets.apply_to(&mut config);
        assert_eq!(config.openrouter_api_key, "sk-or-test");
        assert_eq!(config.telegram_bot_token, "123:abc");
    }
}
