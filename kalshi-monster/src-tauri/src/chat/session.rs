use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

const SESSIONS_DIR: &str = "sessions";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub role: String, // "user", "assistant", "system"
    pub content: String,
    #[serde(default)]
    pub reasoning: Option<String>,
    pub timestamp: String,
    pub tokens_used: Option<u64>,
}

impl ChatMessage {
    pub fn new(role: String, content: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role,
            content,
            reasoning: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            tokens_used: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatSession {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub model: String,
    pub message_count: u32,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ChatState {
    // In-memory cache of session messages
    pub sessions: HashMap<String, Vec<ChatMessage>>,
}

impl ChatState {
    pub fn get_messages(&self, session_id: &str) -> Vec<ChatMessage> {
        self.sessions.get(session_id).cloned().unwrap_or_default()
    }

    pub fn add_message(&mut self, session_id: &str, message: ChatMessage) {
        let entry = self.sessions.entry(session_id.to_string()).or_default();
        entry.push(message);
    }

    pub fn clear_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    /// Load messages from disk into memory cache
    pub fn load_from_disk(&mut self, session_id: &str) {
        if let Ok(messages) = load_session_messages(session_id) {
            self.sessions.insert(session_id.to_string(), messages);
        }
    }
}

/// Base directory for all Kalshi Monster data
fn data_dir() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".openclaw/kalshi-monster")
}

/// Sessions directory within the config dir
fn sessions_dir() -> PathBuf {
    data_dir().join(SESSIONS_DIR)
}

fn ensure_sessions_dir() -> Result<PathBuf, String> {
    let dir = sessions_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create sessions dir: {}", e))?;
    Ok(dir)
}

pub fn create_session(name: Option<String>, model: &str) -> Result<ChatSession, String> {
    let dir = ensure_sessions_dir()?;
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let session_name = name.unwrap_or_else(|| {
        let local_time = chrono::Local::now();
        format!("Predictions {}", local_time.format("%b %d %H:%M"))
    });
    let session = ChatSession {
        id: id.clone(),
        name: session_name,
        created_at: now.clone(),
        updated_at: now,
        model: model.to_string(),
        message_count: 0,
        total_tokens: 0,
    };
    let json = serde_json::to_string_pretty(&session)
        .map_err(|e| format!("Failed to serialize session: {}", e))?;
    fs::write(dir.join(format!("{}.json", id)), json)
        .map_err(|e| format!("Failed to write session: {}", e))?;
    Ok(session)
}

pub fn list_sessions() -> Result<Vec<ChatSession>, String> {
    let dir = sessions_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("Failed to read sessions: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            // Skip message files (they have _messages suffix)
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem.contains("_messages") {
                    continue;
                }
            }
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut session) = serde_json::from_str::<ChatSession>(&content) {
                    // Update message count from messages file
                    if let Ok(msgs) = load_session_messages(&session.id) {
                        session.message_count = msgs.len() as u32;
                        session.total_tokens = msgs.iter().filter_map(|m| m.tokens_used).sum::<u64>();
                    }
                    sessions.push(session);
                }
            }
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}

pub fn delete_session(session_id: &str) -> Result<(), String> {
    let dir = sessions_dir();
    // Delete session metadata
    let session_path = dir.join(format!("{}.json", session_id));
    if session_path.exists() {
        fs::remove_file(&session_path).map_err(|e| format!("Failed to delete session: {}", e))?;
    }
    // Delete messages
    let messages_path = dir.join(format!("{}_messages.json", session_id));
    if messages_path.exists() {
        fs::remove_file(&messages_path).map_err(|e| format!("Failed to delete messages: {}", e))?;
    }
    Ok(())
}

/// Rename a session by rewriting its metadata file with the new name.
pub fn rename_session(session_id: &str, new_name: &str) -> Result<ChatSession, String> {
    let dir = sessions_dir();
    let session_path = dir.join(format!("{}.json", session_id));
    if !session_path.exists() {
        return Err(format!("Session {} not found", session_id));
    }
    let content = fs::read_to_string(&session_path)
        .map_err(|e| format!("Failed to read session: {}", e))?;
    let mut session: ChatSession = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse session: {}", e))?;
    session.name = new_name.to_string();
    session.updated_at = chrono::Utc::now().to_rfc3339();
    let json = serde_json::to_string_pretty(&session)
        .map_err(|e| format!("Failed to serialize session: {}", e))?;
    fs::write(&session_path, json)
        .map_err(|e| format!("Failed to write session: {}", e))?;
    Ok(session)
}

pub fn save_session_messages(
    session_id: &str,
    messages: &[ChatMessage],
) -> Result<(), String> {
    let dir = ensure_sessions_dir()?;
    let path = dir.join(format!("{}_messages.json", session_id));
    let json = serde_json::to_string_pretty(messages)
        .map_err(|e| format!("Failed to serialize messages: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write messages: {}", e))?;

    // Also update session metadata
    let session_path = dir.join(format!("{}.json", session_id));
    if session_path.exists() {
        if let Ok(content) = fs::read_to_string(&session_path) {
            if let Ok(mut session) = serde_json::from_str::<ChatSession>(&content) {
                session.message_count = messages.len() as u32;
                session.total_tokens = messages.iter().filter_map(|m| m.tokens_used).sum::<u64>();
                session.updated_at = chrono::Utc::now().to_rfc3339();
                if let Ok(json) = serde_json::to_string_pretty(&session) {
                    let _ = fs::write(&session_path, json);
                }
            }
        }
    }

    Ok(())
}

pub fn load_session_messages(session_id: &str) -> Result<Vec<ChatMessage>, String> {
    let path = sessions_dir().join(format!("{}_messages.json", session_id));
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("Failed to read messages: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse messages: {}", e))
}
