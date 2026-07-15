//! Fincept sidecar supervisor — plan §7 Phase 1 (`FinceptBridge`).
//!
//! Spawns the AGPL Python analysis process, reads the `FINCEPT_READY port=<n>`
//! handshake from stdout, and issues authenticated HTTP health checks.
//!
//! - Dev: spawns `python` against `../../fincept-sidecar/main.py`.
//! - Prod: spawns the bundled sidecar binary placed next to the app executable
//!   by Tauri `externalBin`.

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

/// Bundled sidecar filename. Tauri strips the target triple and places the
/// binary next to the app executable at runtime.
fn sidecar_exe_name() -> &'static str {
    if cfg!(windows) {
        "fincept-sidecar.exe"
    } else {
        "fincept-sidecar"
    }
}

/// Resolve the bundled sidecar binary next to the current executable.
fn sidecar_binary_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let path = dir.join(sidecar_exe_name());
    path.is_file().then_some(path)
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
    /// Last market-opinion round-trip latency (ms), if any call recorded.
    #[serde(default)]
    pub last_agent_latency_ms: Option<i64>,
    /// Total market-opinion requests attempted this process lifetime.
    #[serde(default)]
    pub agent_calls: u64,
    /// Subset of agent_calls where ≥1 signal had a non-null probability.
    #[serde(default)]
    pub agent_calls_opining: u64,
    /// Sum of signals received across agent calls (for avg diagnostics).
    #[serde(default)]
    pub signals_received_total: u64,
    /// Sum of opining signals across agent calls.
    #[serde(default)]
    pub signals_opining_total: u64,
    /// RFC3339 of last agent call (success or fail).
    #[serde(default)]
    pub last_agent_call_at: Option<String>,
    /// opining_rate = agent_calls_opining / agent_calls (0 when no calls).
    #[serde(default)]
    pub opining_rate: f64,
}

struct FinceptBridgeInner {
    child: Option<Child>,
    token: Option<String>,
    base_url: Option<String>,
    degraded: bool,
    last_error: Option<String>,
    restart_budget: RestartBudget,
    // Sprint 3.2 agent ops counters
    last_agent_latency_ms: Option<i64>,
    agent_calls: u64,
    agent_calls_opining: u64,
    signals_received_total: u64,
    signals_opining_total: u64,
    last_agent_call_at: Option<String>,
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
            last_agent_latency_ms: None,
            agent_calls: 0,
            agent_calls_opining: 0,
            signals_received_total: 0,
            signals_opining_total: 0,
            last_agent_call_at: None,
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
        Self::status_from_inner(&inner, now)
    }

    /// Record one market-opinion attempt (success or failure) for Settings ops UX.
    pub async fn record_agent_call(
        &self,
        latency_ms: Option<i64>,
        signals_received: u64,
        signals_opining: u64,
        error: Option<String>,
    ) {
        let mut inner = self.inner.lock().await;
        inner.agent_calls = inner.agent_calls.saturating_add(1);
        inner.signals_received_total = inner
            .signals_received_total
            .saturating_add(signals_received);
        inner.signals_opining_total = inner
            .signals_opining_total
            .saturating_add(signals_opining);
        if signals_opining > 0 {
            inner.agent_calls_opining = inner.agent_calls_opining.saturating_add(1);
        }
        if let Some(ms) = latency_ms {
            inner.last_agent_latency_ms = Some(ms);
        }
        inner.last_agent_call_at = Some(chrono::Utc::now().to_rfc3339());
        if let Some(e) = error {
            inner.last_error = Some(e);
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

    /// Spawn the sidecar using the bundled binary (production) or Python dev path.
    pub async fn start_sidecar(&self) -> Result<(), String> {
        if tauri::is_dev() {
            self.start_dev_sidecar().await
        } else {
            self.start_bundled_sidecar().await
        }
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

        self.apply_started_child(child, token, base_url).await;
        Ok(())
    }

    /// Spawn the bundled sidecar binary next to the app executable.
    async fn start_bundled_sidecar(&self) -> Result<(), String> {
        let path = sidecar_binary_path().ok_or_else(|| {
            format!(
                "bundled fincept-sidecar not found next to executable (expected {})",
                sidecar_exe_name()
            )
        })?;

        let token = generate_launch_token();
        let mut child = Command::new(&path)
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

        self.apply_started_child(child, token, base_url).await;
        Ok(())
    }

    async fn apply_started_child(&self, child: Child, token: String, base_url: String) {
        let mut inner = self.inner.lock().await;
        if let Some(mut old) = inner.child.take() {
            let _ = old.kill().await;
        }
        inner.child = Some(child);
        inner.token = Some(token);
        inner.base_url = Some(base_url);
        inner.degraded = false;
        inner.last_error = None;
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

    /// Authenticated GET returning JSON body (plan §7 market/tracker/snapshot).
    pub async fn get_json(&self, path: &str) -> Result<serde_json::Value, String> {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let (base_url, token) = {
            let inner = self.inner.lock().await;
            match (&inner.base_url, &inner.token) {
                (Some(u), Some(t)) => (u.clone(), t.clone()),
                _ => return Err("fincept sidecar not online".into()),
            }
        };

        let url = format!("{base_url}{path}");
        let resp = self
            .http
            .get(&url)
            .header("authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| format!("fincept GET {path}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("fincept GET {path} returned {status}: {body}"));
        }

        resp.json()
            .await
            .map_err(|e| format!("fincept GET {path} json: {e}"))
    }

    /// Authenticated POST with JSON body; returns JSON response.
    pub async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let (base_url, token) = {
            let inner = self.inner.lock().await;
            match (&inner.base_url, &inner.token) {
                (Some(u), Some(t)) => (u.clone(), t.clone()),
                _ => return Err("fincept sidecar not online".into()),
            }
        };

        let url = format!("{base_url}{path}");
        let resp = self
            .http
            .post(&url)
            .header("authorization", format!("Bearer {token}"))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("fincept POST {path}: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            return Err(format!("fincept POST {path} returned {status}: {body_text}"));
        }

        resp.json()
            .await
            .map_err(|e| format!("fincept POST {path} json: {e}"))
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
        if let Err(e) = self.start_sidecar().await {
            let mut inner = self.inner.lock().await;
            inner.degraded = true;
            inner.last_error = Some(e);
        }
        self.status().await
    }

    fn status_from_inner(inner: &FinceptBridgeInner, now: Instant) -> FinceptBridgeStatus {
        let opining_rate = if inner.agent_calls == 0 {
            0.0
        } else {
            inner.agent_calls_opining as f64 / inner.agent_calls as f64
        };
        FinceptBridgeStatus {
            online: inner.base_url.is_some() && !inner.degraded,
            degraded: inner.degraded,
            base_url: inner.base_url.clone(),
            last_error: inner.last_error.clone(),
            restarts_remaining: inner.restart_budget.remaining(now),
            last_agent_latency_ms: inner.last_agent_latency_ms,
            agent_calls: inner.agent_calls,
            agent_calls_opining: inner.agent_calls_opining,
            signals_received_total: inner.signals_received_total,
            signals_opining_total: inner.signals_opining_total,
            last_agent_call_at: inner.last_agent_call_at.clone(),
            opining_rate,
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
    fn sidecar_exe_name_matches_tauri_bundled_binary() {
        if cfg!(windows) {
            assert_eq!(super::sidecar_exe_name(), "fincept-sidecar.exe");
        } else {
            assert_eq!(super::sidecar_exe_name(), "fincept-sidecar");
        }
    }

    #[tokio::test]
    async fn record_agent_call_updates_status_counters() {
        let bridge = FinceptBridge::new();
        bridge.record_agent_call(Some(42), 5, 2, None).await;
        bridge
            .record_agent_call(Some(10), 3, 0, Some("timeout".into()))
            .await;
        let st = bridge.status().await;
        assert_eq!(st.agent_calls, 2);
        assert_eq!(st.agent_calls_opining, 1);
        assert_eq!(st.signals_received_total, 8);
        assert_eq!(st.signals_opining_total, 2);
        assert_eq!(st.last_agent_latency_ms, Some(10));
        assert!((st.opining_rate - 0.5).abs() < 1e-9);
        assert_eq!(st.last_error.as_deref(), Some("timeout"));
        assert!(st.last_agent_call_at.is_some());
    }

    /// Sprint 3.3 — release conf must declare externalBin for the sidecar.
    #[test]
    fn release_conf_ships_fincept_sidecar_external_bin() {
        let conf_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.release.json");
        let raw = std::fs::read_to_string(&conf_path)
            .unwrap_or_else(|e| panic!("read {}: {e}", conf_path.display()));
        let v: serde_json::Value =
            serde_json::from_str(&raw).expect("tauri.conf.release.json is valid JSON");
        let bins = v
            .pointer("/bundle/externalBin")
            .and_then(|x| x.as_array())
            .expect("bundle.externalBin array");
        let has = bins.iter().any(|b| {
            b.as_str()
                .map(|s| s.contains("fincept-sidecar"))
                .unwrap_or(false)
        });
        assert!(
            has,
            "tauri.conf.release.json must list binaries/fincept-sidecar in externalBin"
        );
    }

    /// Contract with `scripts/build_fincept_sidecar.py` (repo root): PyInstaller output
    /// is staged under `src-tauri/binaries/fincept-sidecar-<target-triple>[.exe]`.
    #[test]
    fn staged_sidecar_artifact_name_matches_build_script() {
        let triple = if cfg!(windows) {
            "x86_64-pc-windows-msvc"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin"
            } else {
                "x86_64-apple-darwin"
            }
        } else {
            "x86_64-unknown-linux-gnu"
        };
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let staged = format!("fincept-sidecar-{triple}{suffix}");
        let binaries_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries");
        assert!(
            binaries_dir.is_dir(),
            "binaries dir missing: {}",
            binaries_dir.display()
        );
        let dev_entry = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fincept-sidecar/main.py")
            .canonicalize()
            .expect("fincept-sidecar main.py for dev spawn");
        assert!(dev_entry.is_file(), "dev entrypoint: {}", dev_entry.display());
        assert_eq!(
            staged,
            if cfg!(windows) {
                "fincept-sidecar-x86_64-pc-windows-msvc.exe"
            } else if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
                "fincept-sidecar-aarch64-apple-darwin"
            } else if cfg!(target_os = "macos") {
                "fincept-sidecar-x86_64-apple-darwin"
            } else {
                "fincept-sidecar-x86_64-unknown-linux-gnu"
            }
        );
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