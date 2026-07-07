//! Fincept sidecar supervisor — plan §7 Phase 1 (`FinceptBridge`).
//!
//! Spawns the AGPL Python analysis process, reads the `FINCEPT_READY port=<n>`
//! handshake from stdout, and issues authenticated HTTP health checks. Full
//! Tauri `sidecar()` packaging is deferred until `externalBin` is wired; dev
//! launches use `python` against `../../fincept-sidecar/main.py`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time;

pub const READY_LINE_PREFIX: &str = "FINCEPT_READY port=";
pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_RESTARTS_PER_WINDOW: u32 = 3;
pub const RESTART_WINDOW: Duration = Duration::from_secs(600);

/// Parse `FINCEPT_READY port=<n>` from a single stdout line (no trailing noise).
pub fn parse_ready_line(line: &str) -> Option<u16> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix(READY_LINE_PREFIX)?;
    rest.parse().ok().filter(|&p| p > 0)
}

/// Per-launch bearer secret for sidecar auth (plan §10.2).
pub fn generate_launch_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Tracks restart attempts inside a rolling window (plan: max 3 / 10 min).
#[derive(Debug, Clone, Default)]
pub struct RestartBudget {
    window_start: Option<Instant>,
    count: u32,
}

impl RestartBudget {
    pub fn record_restart(&mut self, now: Instant) -> bool {
        let start = self.window_start.get_or_insert(now);
        if now.duration_since(*start) >= RESTART_WINDOW {
            *start = now;
            self.count = 0;
        }
        if self.count >= MAX_RESTARTS_PER_WINDOW {
            return false;
        }
        self.count += 1;
        true
    }

    pub fn remaining(&self, now: Instant) -> u32 {
        let Some(start) = self.window_start else {
            return MAX_RESTARTS_PER_WINDOW;
        };
        if now.duration_since(start) >= RESTART_WINDOW {
            return MAX_RESTARTS_PER_WINDOW;
        }
        MAX_RESTARTS_PER_WINDOW.saturating_sub(self.count)
    }

    pub fn is_exhausted(&self, now: Instant) -> bool {
        self.remaining(now) == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinceptBridgeStatus {
    pub online: bool,
    pub degraded: bool,
    pub base_url: Option<String>,
    pub last_error: Option<String>,
    pub restarts_remaining: u32,
}

struct FinceptBridgeInner {
    child: Option<Child>,
    token: Option<String>,
    base_url: Option<String>,
    degraded: bool,
    last_error: Option<String>,
    restart_budget: RestartBudget,
}

impl Default for FinceptBridgeInner {
    fn default() -> Self {
        Self {
            child: None,
            token: None,
            base_url: None,
            degraded: false,
            last_error: None,
            restart_budget: RestartBudget::default(),
        }
    }
}

pub struct FinceptBridge {
    inner: Mutex<FinceptBridgeInner>,
    http: reqwest::Client,
}

impl FinceptBridge {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            inner: Mutex::new(FinceptBridgeInner::default()),
            http,
        }
    }

    pub async fn status(&self) -> FinceptBridgeStatus {
        let inner = self.inner.lock().await;
        let now = Instant::now();
        FinceptBridgeStatus {
            online: inner.base_url.is_some() && !inner.degraded,
            degraded: inner.degraded,
            base_url: inner.base_url.clone(),
            last_error: inner.last_error.clone(),
            restarts_remaining: inner.restart_budget.remaining(now),
        }
    }

    /// Resolve `fincept-sidecar/main.py` relative to the Tauri crate (dev layout).
    pub fn default_main_py_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fincept-sidecar/main.py")
            .canonicalize()
            .unwrap_or_else(|_| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fincept-sidecar/main.py")
            })
    }

    /// Spawn the Python sidecar and block until READY or timeout.
    pub async fn start_dev_sidecar(&self) -> Result<(), String> {
        let main_py = Self::default_main_py_path();
        if !main_py.is_file() {
            return Err(format!(
                "fincept-sidecar main.py not found at {}",
                main_py.display()
            ));
        }

        let token = generate_launch_token();
        let python = std::env::var("FINCEPT_PYTHON").unwrap_or_else(|_| "python".into());
        let sidecar_dir = main_py
            .parent()
            .ok_or_else(|| "invalid fincept-sidecar path".to_string())?;

        let mut child = Command::new(&python)
            .arg(&main_py)
            .current_dir(sidecar_dir)
            .env("FINCEPT_TOKEN", &token)
            .env("FINCEPT_PORT", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("spawn fincept-sidecar: {e}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "sidecar stdout not piped".to_string())?;

        let port = wait_for_ready_port(stdout, DEFAULT_HANDSHAKE_TIMEOUT).await?;
        let base_url = format!("http://127.0.0.1:{port}");

        let mut inner = self.inner.lock().await;
        if let Some(mut old) = inner.child.take() {
            let _ = old.kill().await;
        }
        inner.child = Some(child);
        inner.token = Some(token);
        inner.base_url = Some(base_url);
        inner.degraded = false;
        inner.last_error = None;
        Ok(())
    }

    pub async fn stop(&self) {
        let mut inner = self.inner.lock().await;
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill().await;
        }
        inner.base_url = None;
        inner.token = None;
    }

    /// GET `/api/v1/health` with the per-launch bearer token.
    pub async fn health_check(&self) -> Result<bool, String> {
        let (base_url, token) = {
            let inner = self.inner.lock().await;
            match (&inner.base_url, &inner.token) {
                (Some(u), Some(t)) => (u.clone(), t.clone()),
                _ => return Ok(false),
            }
        };

        let url = format!("{base_url}/api/v1/health");
        let resp = self
            .http
            .get(&url)
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| format!("health request failed: {e}"))?;

        Ok(resp.status().is_success())
    }

    /// On repeated health failures, attempt restart until budget exhausted → degraded.
    pub async fn record_health_failure(&self) -> FinceptBridgeStatus {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        inner.last_error = Some("health check failed".into());

        if inner.restart_budget.is_exhausted(now) {
            inner.degraded = true;
            return Self::status_from_inner(&inner, now);
        }

        if !inner.restart_budget.record_restart(now) {
            inner.degraded = true;
            return Self::status_from_inner(&inner, now);
        }

        drop(inner);
        if let Err(e) = self.start_dev_sidecar().await {
            let mut inner = self.inner.lock().await;
            inner.degraded = true;
            inner.last_error = Some(e);
        }
        self.status().await
    }

    fn status_from_inner(inner: &FinceptBridgeInner, now: Instant) -> FinceptBridgeStatus {
        FinceptBridgeStatus {
            online: inner.base_url.is_some() && !inner.degraded,
            degraded: inner.degraded,
            base_url: inner.base_url.clone(),
            last_error: inner.last_error.clone(),
            restarts_remaining: inner.restart_budget.remaining(now),
        }
    }
}

async fn wait_for_ready_port(
    stdout: impl tokio::io::AsyncRead + Unpin,
    timeout: Duration,
) -> Result<u16, String> {
    let mut reader = BufReader::new(stdout).lines();
    let deadline = time::sleep(timeout);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => {
                return Err(format!(
                    "timed out waiting for {READY_LINE_PREFIX}<port> ({timeout:?})"
                ));
            }
            line = reader.next_line() => {
                let line = line.map_err(|e| format!("read sidecar stdout: {e}"))?;
                let Some(line) = line else {
                    return Err("sidecar exited before READY line".into());
                };
                if let Some(port) = parse_ready_line(&line) {
                    return Ok(port);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ready_line_accepts_valid() {
        assert_eq!(parse_ready_line("FINCEPT_READY port=54321"), Some(54321));
        assert_eq!(
            parse_ready_line("  FINCEPT_READY port=8080  \n"),
            Some(8080)
        );
    }

    #[test]
    fn parse_ready_line_rejects_invalid() {
        assert_eq!(parse_ready_line("listening on 8080"), None);
        assert_eq!(parse_ready_line("FINCEPT_READY port=0"), None);
        assert_eq!(parse_ready_line("FINCEPT_READY port=abc"), None);
    }

    #[test]
    fn restart_budget_caps_at_three_per_window() {
        let mut b = RestartBudget::default();
        let t0 = Instant::now();
        assert!(b.record_restart(t0));
        assert!(b.record_restart(t0));
        assert!(b.record_restart(t0));
        assert!(!b.record_restart(t0));
        assert_eq!(b.remaining(t0), 0);
        assert!(b.is_exhausted(t0));
    }

    #[test]
    fn restart_budget_resets_after_window() {
        let mut b = RestartBudget::default();
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(b.record_restart(t0));
        }
        assert!(!b.record_restart(t0));
        let t1 = t0 + RESTART_WINDOW + Duration::from_secs(1);
        assert_eq!(b.remaining(t1), MAX_RESTARTS_PER_WINDOW);
        assert!(b.record_restart(t1));
    }

    /// Spawns real fincept-sidecar when Python + deps are available (dev/CI).
    #[tokio::test]
    #[ignore = "requires fincept-sidecar on disk and python deps"]
    async fn dev_sidecar_handshake_integration() {
        let bridge = FinceptBridge::new();
        bridge
            .start_dev_sidecar()
            .await
            .expect("start_dev_sidecar");
        assert!(bridge.health_check().await.expect("health"));
        bridge.stop().await;
    }
}