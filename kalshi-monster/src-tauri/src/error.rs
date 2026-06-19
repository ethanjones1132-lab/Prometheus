use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("API error: {0}")]
    Api(String),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

#[derive(Debug, Serialize, Clone)]
pub struct ErrorResponse {
    pub error_type: String,
    pub message: String,
}

impl From<AppError> for String {
    fn from(err: AppError) -> String {
        let error_type = match &err {
            AppError::Config(_) => "Config",
            AppError::Database(_) => "Database",
            AppError::Network(_) => "Network",
            AppError::Api(_) => "Api",
            AppError::Auth(_) => "Auth",
            AppError::Validation(_) => "Validation",
            AppError::NotFound(_) => "NotFound",
            AppError::Internal(_) => "Internal",
            AppError::Io(_) => "Io",
            AppError::Serialization(_) => "Serialization",
        };
        let response = ErrorResponse {
            error_type: error_type.to_string(),
            message: err.to_string(),
        };
        serde_json::to_string(&response).unwrap_or_else(|_| format!("{{\"error_type\":\"{}\",\"message\":\"{}\"}}", error_type, err))
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Serialization(err.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            AppError::Network(format!("Request timed out: {}", err))
        } else if err.is_connect() {
            AppError::Network(format!("Connection failed: {}", err))
        } else {
            AppError::Api(err.to_string())
        }
    }
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        let lower = err.to_lowercase();
        if lower.contains("timeout") || lower.contains("connection") || lower.contains("network") {
            AppError::Network(err)
        } else if lower.contains("config") || lower.contains("save_config") || lower.contains("load_config") {
            AppError::Config(err)
        } else if lower.contains("auth") || lower.contains("login") || lower.contains("credential") || lower.contains("unauthorized") || lower.contains("401") || lower.contains("403") {
            AppError::Auth(err)
        } else if lower.contains("not found") || lower.contains("404") {
            AppError::NotFound(err)
        } else if lower.contains("database") || lower.contains("sqlite") || lower.contains("sqlx") {
            AppError::Database(err)
        } else if lower.contains("invalid") || lower.contains("validation") || lower.contains("parse") {
            AppError::Validation(err)
        } else {
            AppError::Internal(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_messages() {
        let err = AppError::Config("bad config".to_string());
        assert_eq!(err.to_string(), "Configuration error: bad config");

        let err = AppError::Network("timeout".to_string());
        assert_eq!(err.to_string(), "Network error: timeout");

        let err = AppError::Validation("invalid input".to_string());
        assert_eq!(err.to_string(), "Validation error: invalid input");

        let err = AppError::NotFound("missing resource".to_string());
        assert_eq!(err.to_string(), "Not found: missing resource");

        let err = AppError::Database("sqlite error".to_string());
        assert_eq!(err.to_string(), "Database error: sqlite error");
    }

    #[test]
    fn test_error_into_string_json() {
        let err = AppError::Api("rate limited".to_string());
        let json_str: String = err.into();
        assert!(json_str.contains("\"error_type\":\"Api\""));
        assert!(json_str.contains("rate limited"));
    }

    #[test]
    fn test_string_to_app_error_classification() {
        assert!(matches!(
            AppError::from("connection refused".to_string()),
            AppError::Network(_)
        ));
        assert!(matches!(
            AppError::from("config file missing".to_string()),
            AppError::Config(_)
        ));
        assert!(matches!(
            AppError::from("unauthorized access".to_string()),
            AppError::Auth(_)
        ));
        assert!(matches!(
            AppError::from("not found in database".to_string()),
            AppError::NotFound(_)
        ));
        assert!(matches!(
            AppError::from("invalid parameter".to_string()),
            AppError::Validation(_)
        ));
        assert!(matches!(
            AppError::from("something went wrong".to_string()),
            AppError::Internal(_)
        ));
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let app_err: AppError = io_err.into();
        assert!(matches!(app_err, AppError::Io(_)));
    }
}
