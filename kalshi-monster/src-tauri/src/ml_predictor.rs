#![allow(dead_code)]
// ═══════════════════════════════════════════════════════════════
// ML Predictor — Python-interop ML training & inference engine
//
// Bridges the SQLite prediction store with a Python scikit-learn
// model (GradientBoosting classifier) for prop outcome prediction.
//
// Flow:
//   1. Rust extracts features from SQLite (predictions + line movements)
//   2. Shells out to ml_predictor.py for training and inference
//   3. Stores ML predictions back in SQLite for frontend display
//   4. Injects ML context into the AI chat prompt
//
// The Python script lives at:
//   src-tauri/src/ml_predictor.py
// ═══════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use sqlx::{Pool, Row, Sqlite};
use std::fmt::Write;
use std::path::PathBuf;
use std::process::Command;

// ═══════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════

/// ML model training result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLTrainingResult {
    pub status: String,
    pub samples: Option<i64>,
    pub cv_accuracy_mean: Option<f64>,
    pub cv_accuracy_std: Option<f64>,
    pub win_rate: Option<f64>,
    pub model_path: Option<String>,
    pub feature_importance: Option<Vec<MLFeatureImportance>>,
    pub message: String,
    #[serde(default)]
    pub category_breakdown: Option<std::collections::HashMap<String, i64>>,
}

/// Feature importance from the trained model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLFeatureImportance {
    pub feature: String,
    pub importance: f64,
}

/// A single ML prediction for a pending prop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLPrediction {
    pub prediction_id: String,
    pub player_name: String,
    pub stat_category: String,
    pub line: f64,
    pub ml_win_probability: f64,
    pub ml_prediction: String,
    pub original_confidence: i64,
    pub original_probability: Option<f64>,
    pub line_change: f64,
    #[serde(default)]
    pub category_code: Option<i64>,
}

/// Batch prediction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLPredictionBatch {
    pub status: String,
    pub model_path: Option<String>,
    pub predictions_count: i64,
    pub predictions: Vec<MLPrediction>,
    pub message: String,
}

/// Resolved prediction counts per market category (for multi-category ML readiness)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLCategoryStats {
    pub category: String,
    pub resolved_count: i64,
    pub pending_count: i64,
    /// True when resolved_count >= min for a dedicated per-category model
    pub trainable: bool,
    /// Graded samples still needed before a per-category sidecar can train
    pub samples_until_trainable: i64,
    /// Threshold used for `trainable` (exported for Settings UI)
    pub min_resolved_for_sidecar: i64,
}

/// Per-category sidecar model summary (politics/econ/weather)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLPerCategoryModel {
    pub samples: i64,
    pub cv_accuracy_mean: Option<f64>,
    pub model_exists: bool,
}

/// ML model status for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLModelStatus {
    pub model_exists: bool,
    pub model_path: String,
    pub trained_at: Option<String>,
    pub samples: Option<i64>,
    pub cv_accuracy_mean: Option<f64>,
    pub cv_accuracy_std: Option<f64>,
    pub win_rate: Option<f64>,
    pub feature_importance: Option<Vec<MLFeatureImportance>>,
    pub pending_predictions: i64,
    pub resolved_predictions: i64,
    /// Live DB counts by category (Kalshi politics/econ/weather + sports)
    #[serde(default)]
    pub category_stats: Vec<MLCategoryStats>,
    /// Last training set mix (from model _meta.json when available)
    #[serde(default)]
    pub training_category_breakdown: Option<std::collections::HashMap<String, i64>>,
    /// Trained sidecar classifiers per non-sports category (when 10+ graded samples)
    #[serde(default)]
    pub per_category_models: Option<std::collections::HashMap<String, MLPerCategoryModel>>,
    /// Politics/Economics/Weather categories with ≥10 graded rows (Phase 3 ROADMAP)
    #[serde(default)]
    pub trainable_non_sports_categories: i64,
    /// Target count for Phase 3 multi-category ML success metric
    #[serde(default = "default_non_sports_sidecar_target")]
    pub non_sports_sidecar_target: i64,
    /// Non-sports category closest to the 10-sample sidecar threshold (UX hint)
    #[serde(default)]
    pub next_sidecar_category: Option<String>,
    #[serde(default)]
    pub next_sidecar_samples_needed: Option<i64>,
    /// True when total resolved rows meet the auto-retrain-after-grade threshold (≥10).
    #[serde(default)]
    pub auto_retrain_eligible: bool,
    /// Additional resolved predictions needed before auto-retrain can run after grading.
    #[serde(default)]
    pub resolved_until_auto_retrain: i64,
    pub message: String,
}

fn default_non_sports_sidecar_target() -> i64 {
    3
}

const NON_SPORTS_SIDECAR_CATEGORIES: &[&str] = &["Politics", "Economics", "Weather"];

/// Count non-sports categories that meet the sidecar training threshold (ROADMAP Phase 3).
pub(crate) fn count_trainable_non_sports_categories(stats: &[MLCategoryStats]) -> i64 {
    stats
        .iter()
        .filter(|s| {
            s.trainable && NON_SPORTS_SIDECAR_CATEGORIES.contains(&s.category.as_str())
        })
        .count() as i64
}

fn zero_sidecar_stat(category: &str) -> MLCategoryStats {
    MLCategoryStats {
        category: category.to_string(),
        resolved_count: 0,
        pending_count: 0,
        trainable: false,
        samples_until_trainable: MIN_CATEGORY_TRAIN_SAMPLES,
        min_resolved_for_sidecar: MIN_CATEGORY_TRAIN_SAMPLES,
    }
}

/// Ensure Politics/Economics/Weather rows exist so Settings always shows Phase 3 targets.
pub(crate) fn ensure_non_sports_sidecar_stats(mut stats: Vec<MLCategoryStats>) -> Vec<MLCategoryStats> {
    for cat in NON_SPORTS_SIDECAR_CATEGORIES {
        if !stats.iter().any(|s| s.category == *cat) {
            stats.push(zero_sidecar_stat(cat));
        }
    }
    stats.sort_by(|a, b| {
        let a_target = NON_SPORTS_SIDECAR_CATEGORIES.contains(&a.category.as_str());
        let b_target = NON_SPORTS_SIDECAR_CATEGORIES.contains(&b.category.as_str());
        b_target
            .cmp(&a_target)
            .then_with(|| b.resolved_count.cmp(&a.resolved_count))
            .then_with(|| a.category.cmp(&b.category))
    });
    stats
}

/// Pick the non-sports category that needs the fewest additional graded rows for a sidecar.
pub(crate) fn nearest_non_sports_sidecar_unlock(
    stats: &[MLCategoryStats],
) -> Option<(String, i64)> {
    stats
        .iter()
        .filter(|s| {
            NON_SPORTS_SIDECAR_CATEGORIES.contains(&s.category.as_str()) && !s.trainable
        })
        .min_by_key(|s| s.samples_until_trainable)
        .map(|s| (s.category.clone(), s.samples_until_trainable))
}

/// ML-enhanced analysis context — extends the existing AnalysisContext
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLAnalysisContext {
    pub ml_predictions: Vec<MLPrediction>,
    pub model_accuracy: Option<f64>,
    pub model_samples: Option<i64>,
}

// ═══════════════════════════════════════════════════════════════
// Paths
// ═══════════════════════════════════════════════════════════════

/// Path to the Python ML script
fn ml_script_path() -> PathBuf {
    // In production, this is relative to the app bundle.
    // For development, use the source directory.
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        PathBuf::from(manifest).join("src/ml_predictor.py")
    } else {
        PathBuf::from("src/ml_predictor.py")
    }
}

/// Default model output path
fn default_model_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".openclaw/kalshi-monster/ml_model.joblib")
}

/// Default predictions db path
pub fn default_db_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".openclaw/kalshi-monster/predictions.db")
}

/// Model metadata path
fn model_meta_path(model_path: &PathBuf) -> PathBuf {
    model_path.with_file_name(format!(
        "{}_meta.json",
        model_path.file_stem().unwrap_or_default().to_string_lossy()
    ))
}

// ═══════════════════════════════════════════════════════════════
// Core Operations
// ═══════════════════════════════════════════════════════════════

/// Train the ML model on historical prediction data
pub async fn train_model(
    db_path: Option<&str>,
    output_path: Option<&str>,
) -> Result<MLTrainingResult, String> {
    let db = db_path.map(PathBuf::from).unwrap_or_else(default_db_path);
    let output = output_path
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let script = ml_script_path();

    if !script.exists() {
        return Err(format!(
            "ML script not found at {}. Ensure ml_predictor.py is in the src/ directory.",
            script.display()
        ));
    }

    let output_str = output.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("train")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--output")
            .arg(&output_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if !out.status.success() {
            return Err(format!(
                "ml_predictor.py failed (exit {}): {}",
                out.status, stderr
            ));
        }

        // Parse JSON output (last line)
        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        let result: MLTrainingResult = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse ml_predictor output: {}\nRaw: {}", e, json_line))?;

        Ok::<_, String>(result)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result)
}

/// Retrain unified + per-category sidecars after Kalshi auto-grader resolves markets.
pub async fn retrain_after_grading(graded_count: u32, pool: Option<&Pool<Sqlite>>) {
    let resolved = if let Some(p) = pool {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM predictions WHERE outcome IN ('Win', 'Loss', 'Push')",
        )
        .fetch_one(p)
        .await
        .unwrap_or(0)
    } else {
        MIN_CATEGORY_TRAIN_SAMPLES
    };
    if !should_retrain_given_resolved(graded_count, resolved) {
        tracing::debug!(
            "ml: skip retrain after grading (graded={}, resolved={}, need >=10 resolved)",
            graded_count,
            resolved
        );
        return;
    }
    match train_model(None, None).await {
        Ok(r) if r.status == "trained" => {
            tracing::info!(
                "ml: retrained after {} new grades — {} samples, CV {:.1}%",
                graded_count,
                r.samples.unwrap_or(0),
                r.cv_accuracy_mean.unwrap_or(0.0) * 100.0
            );
        }
        Ok(r) => {
            tracing::info!("ml: retrain after grading: {}", r.message);
        }
        Err(e) => {
            tracing::debug!("ml: retrain after grading skipped: {}", e);
        }
    }
}

/// Gate for background ML retrain after auto-grade (testable, no Python spawn).
pub(crate) fn should_retrain_after_grading(graded_count: u32) -> bool {
    graded_count > 0
}

/// Generate ML predictions for all pending props
pub async fn predict_batch(
    db_path: Option<&str>,
    model_path: Option<&str>,
) -> Result<MLPredictionBatch, String> {
    let db = db_path.map(PathBuf::from).unwrap_or_else(default_db_path);
    let model = model_path
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let script = ml_script_path();

    if !script.exists() {
        return Err(format!("ML script not found at {}", script.display()));
    }

    if !model.exists() {
        return Ok(MLPredictionBatch {
            status: "no_model".to_string(),
            model_path: None,
            predictions_count: 0,
            predictions: vec![],
            message: "Model not found. Train first using ml_train.".to_string(),
        });
    }

    let model_str = model.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("predict")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--model")
            .arg(&model_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if !out.status.success() {
            return Err(format!(
                "ml_predictor.py failed (exit {}): {}",
                out.status, stderr
            ));
        }

        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        #[derive(Deserialize)]
        struct RawBatch {
            status: String,
            model_path: Option<String>,
            predictions_count: i64,
            predictions: Vec<MLPrediction>,
        }

        let raw: RawBatch = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse prediction output: {}", e))?;

        let message = match raw.status.as_str() {
            "ok" => format!("Generated {} ML predictions", raw.predictions_count),
            "no_pending" => "No pending predictions to score".to_string(),
            "no_model" => "Model not found. Train first.".to_string(),
            _ => format!("Status: {}", raw.status),
        };

        Ok::<_, String>(MLPredictionBatch {
            status: raw.status,
            model_path: raw.model_path,
            predictions_count: raw.predictions_count,
            predictions: raw.predictions,
            message,
        })
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result)
}

/// Minimum resolved samples before a per-category dedicated model is considered viable
const MIN_CATEGORY_TRAIN_SAMPLES: i64 = 10;

/// Full gate: new grades landed and enough resolved rows for unified training (≥10).
pub(crate) fn should_retrain_given_resolved(graded_count: u32, resolved_predictions: i64) -> bool {
    graded_count > 0 && auto_retrain_eligible(resolved_predictions)
}

/// Whether total resolved predictions satisfy the unified auto-retrain threshold.
pub(crate) fn auto_retrain_eligible(resolved_predictions: i64) -> bool {
    resolved_predictions >= MIN_CATEGORY_TRAIN_SAMPLES
}

/// Resolved rows still needed before auto-retrain can run (0 when eligible).
pub(crate) fn resolved_until_auto_retrain(resolved_predictions: i64) -> i64 {
    (MIN_CATEGORY_TRAIN_SAMPLES - resolved_predictions).max(0)
}

/// Compact training summary for LLM system prompts (CV ± std, active sidecars).
pub(crate) fn format_ml_training_header(status: &MLModelStatus) -> String {
    let acc = status
        .cv_accuracy_mean
        .map(|a| {
            if let Some(std) = status.cv_accuracy_std {
                format!("{:.1}% ± {:.1}%", a * 100.0, std * 100.0)
            } else {
                format!("{:.1}%", a * 100.0)
            }
        })
        .unwrap_or_else(|| "N/A".to_string());
    let samples = status
        .samples
        .map(|s| s.to_string())
        .unwrap_or_else(|| "N/A".to_string());
    let mut line = format!("trained on {} samples, CV accuracy: {}", samples, acc);
    if let Some(ref per) = status.per_category_models {
        let names: Vec<&str> = per
            .iter()
            .filter(|(_, v)| v.model_exists)
            .map(|(k, _)| k.as_str())
            .collect();
        if !names.is_empty() {
            let _ = write!(line, "; active sidecars: {}", names.join(", "));
        }
    }
    line
}

/// Aggregate resolved/pending counts by market category for multi-category ML readiness
async fn fetch_category_stats(pool: &Pool<Sqlite>) -> Vec<MLCategoryStats> {
    let rows = sqlx::query(
        r#"
        SELECT
            COALESCE(
                NULLIF(json_extract(full_decision_json, '$.category'), ''),
                NULLIF(stat_category, ''),
                'Sports'
            ) AS category,
            SUM(CASE WHEN outcome IN ('Win', 'Loss', 'Push') THEN 1 ELSE 0 END) AS resolved_count,
            SUM(CASE WHEN outcome = 'Pending' THEN 1 ELSE 0 END) AS pending_count
        FROM predictions
        WHERE line IS NOT NULL OR full_decision_json IS NOT NULL
        GROUP BY category
        ORDER BY resolved_count DESC, category ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.iter()
        .map(|r| {
            let resolved: i64 = r.try_get("resolved_count").unwrap_or(0);
            let until = (MIN_CATEGORY_TRAIN_SAMPLES - resolved).max(0);
            MLCategoryStats {
                category: r.try_get("category").unwrap_or_else(|_| "Other".to_string()),
                resolved_count: resolved,
                pending_count: r.try_get("pending_count").unwrap_or(0),
                trainable: resolved >= MIN_CATEGORY_TRAIN_SAMPLES,
                samples_until_trainable: until,
                min_resolved_for_sidecar: MIN_CATEGORY_TRAIN_SAMPLES,
            }
        })
        .collect()
}

fn format_category_readiness(stats: &[MLCategoryStats]) -> String {
    if stats.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = stats
        .iter()
        .map(|s| {
            let flag = if s.trainable {
                "ready".to_string()
            } else {
                format!(
                    "{}/{} graded ({} more for sidecar)",
                    s.resolved_count,
                    s.min_resolved_for_sidecar,
                    s.samples_until_trainable
                )
            };
            format!("{}: {}", s.category, flag)
        })
        .collect();
    format!(" Category mix: {}.", parts.join("; "))
}

/// Get ML model status including training metadata
pub async fn get_model_status(
    pool: &Pool<Sqlite>,
    model_path: Option<&str>,
) -> Result<MLModelStatus, String> {
    let model = model_path
        .map(PathBuf::from)
        .unwrap_or_else(default_model_path);
    let meta_path = model_meta_path(&model);

    let model_exists = model.exists();

    // Load metadata if available
    let (
        trained_at,
        samples,
        cv_mean,
        cv_std,
        win_rate,
        feature_importance,
        training_category_breakdown,
        per_category_models,
    ) = if meta_path.exists() {
        let content = std::fs::read_to_string(&meta_path)
            .map_err(|e| format!("Failed to read model meta: {}", e))?;
        #[derive(Deserialize)]
        struct CatMetaRaw {
            samples: i64,
            #[serde(default)]
            cv_accuracy_mean: Option<f64>,
            #[serde(default)]
            model_path: Option<String>,
        }
        #[derive(Deserialize)]
        struct Meta {
            trained_at: String,
            samples: i64,
            cv_accuracy_mean: f64,
            cv_accuracy_std: f64,
            win_rate: f64,
            feature_importance: Vec<MLFeatureImportance>,
            #[serde(default)]
            category_breakdown: Option<std::collections::HashMap<String, i64>>,
            #[serde(default)]
            per_category_models: Option<std::collections::HashMap<String, CatMetaRaw>>,
        }
        match serde_json::from_str::<Meta>(&content) {
            Ok(m) => {
                let per_cat = m.per_category_models.map(|raw| {
                    raw.into_iter()
                        .map(|(name, info)| {
                            let path = info.model_path.as_ref().map(PathBuf::from);
                            let exists = path
                                .as_ref()
                                .map(|p| p.exists())
                                .unwrap_or_else(|| {
                                    let stem = model
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy();
                                    let sidecar = model.with_file_name(format!(
                                        "{}_{}.joblib",
                                        stem,
                                        name.to_lowercase()
                                    ));
                                    sidecar.exists()
                                });
                            (
                                name,
                                MLPerCategoryModel {
                                    samples: info.samples,
                                    cv_accuracy_mean: info.cv_accuracy_mean,
                                    model_exists: exists,
                                },
                            )
                        })
                        .collect()
                });
                (
                    Some(m.trained_at),
                    Some(m.samples),
                    Some(m.cv_accuracy_mean),
                    Some(m.cv_accuracy_std),
                    Some(m.win_rate),
                    Some(m.feature_importance),
                    m.category_breakdown,
                    per_cat,
                )
            }
            Err(_) => (None, None, None, None, None, None, None, None),
        }
    } else {
        (None, None, None, None, None, None, None, None)
    };

    let category_stats = ensure_non_sports_sidecar_stats(fetch_category_stats(pool).await);

    // Count predictions
    let pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM predictions WHERE outcome = 'Pending'",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let resolved: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM predictions WHERE outcome IN ('Win', 'Loss', 'Push')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let readiness = format_category_readiness(&category_stats);
    let message = if !model_exists {
        format!(
            "No model trained yet. Need at least 10 resolved predictions to train.{}",
            readiness
        )
    } else if let Some(s) = samples {
        format!(
            "Model trained on {} samples. CV accuracy: {:.1}%.{}",
            s,
            cv_mean.unwrap_or(0.0) * 100.0,
            readiness
        )
    } else {
        format!(
            "Model file exists but metadata is missing. Retrain for best results.{}",
            readiness
        )
    };

    let trainable_non_sports = count_trainable_non_sports_categories(&category_stats);
    let next_unlock = nearest_non_sports_sidecar_unlock(&category_stats);

    Ok(MLModelStatus {
        model_exists,
        model_path: model.display().to_string(),
        trained_at,
        samples,
        cv_accuracy_mean: cv_mean,
        cv_accuracy_std: cv_std,
        win_rate,
        feature_importance,
        pending_predictions: pending,
        resolved_predictions: resolved,
        category_stats,
        training_category_breakdown,
        per_category_models,
        trainable_non_sports_categories: trainable_non_sports,
        non_sports_sidecar_target: default_non_sports_sidecar_target(),
        next_sidecar_category: next_unlock.as_ref().map(|(c, _)| c.clone()),
        next_sidecar_samples_needed: next_unlock.map(|(_, n)| n),
        auto_retrain_eligible: auto_retrain_eligible(resolved),
        resolved_until_auto_retrain: resolved_until_auto_retrain(resolved),
        message,
    })
}

/// Store ML predictions in the database for frontend display
/// Adds ml_win_probability to a dedicated table
pub async fn init_ml_tables(pool: &Pool<Sqlite>) -> Result<(), String> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS ml_predictions (
            id TEXT PRIMARY KEY,
            prediction_id TEXT NOT NULL,
            ml_win_probability REAL NOT NULL,
            ml_prediction TEXT NOT NULL,
            ml_model_version TEXT,
            created_at TEXT NOT NULL,
            FOREIGN KEY (prediction_id) REFERENCES predictions(id)
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| format!("Failed to create ml_predictions table: {}", e))?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ml_pred_prediction ON ml_predictions(prediction_id)",
    )
    .execute(pool)
    .await
    .ok();

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_ml_pred_created ON ml_predictions(created_at)",
    )
    .execute(pool)
    .await
    .ok();

    // Legacy index referenced a non-existent ticker column; safe no-op on fresh DBs.
    sqlx::query("DROP INDEX IF EXISTS idx_ml_pred_ticker")
        .execute(pool)
        .await
        .ok();

    Ok(())
}

/// Save a batch of ML predictions to the database
pub async fn save_ml_predictions(
    pool: &Pool<Sqlite>,
    predictions: &[MLPrediction],
    model_version: &str,
) -> Result<usize, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut saved = 0;

    for pred in predictions {
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO ml_predictions
                (id, prediction_id, ml_win_probability, ml_prediction, ml_model_version, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(&id)
        .bind(&pred.prediction_id)
        .bind(pred.ml_win_probability)
        .bind(&pred.ml_prediction)
        .bind(model_version)
        .bind(&now)
        .execute(pool)
        .await
        .map_err(|e| format!("Failed to save ML prediction: {}", e))?;
        saved += 1;
    }

    Ok(saved)
}

/// Get stored ML predictions for display
pub async fn get_stored_ml_predictions(
    pool: &Pool<Sqlite>,
    limit: i64,
) -> Result<Vec<MLPrediction>, String> {
    let rows = sqlx::query(
        r#"
        SELECT mp.prediction_id, p.player_name, p.stat_category, p.line,
               mp.ml_win_probability, mp.ml_prediction,
               p.confidence_score, p.probability
        FROM ml_predictions mp
        JOIN predictions p ON mp.prediction_id = p.id
        ORDER BY mp.created_at DESC
        LIMIT ?1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Failed to fetch ML predictions: {}", e))?;

    Ok(rows
        .iter()
        .map(|r| MLPrediction {
            prediction_id: r.get("prediction_id"),
            player_name: r.get("player_name"),
            stat_category: r.get("stat_category"),
            line: r.get("line"),
            ml_win_probability: r.get("ml_win_probability"),
            ml_prediction: r.get("ml_prediction"),
            original_confidence: r.get::<Option<i64>, _>("confidence_score").unwrap_or(50),
            original_probability: r.get("probability"),
            line_change: 0.0,  // not stored in ml_predictions table
            category_code: None,
        })
        .collect())
}

/// Generate ML context string for AI prompt injection
pub fn generate_ml_context(predictions: &[MLPrediction], accuracy: Option<f64>) -> String {
    if predictions.is_empty() {
        return String::new();
    }

    let acc_str = accuracy.map_or("N/A".to_string(), |a| format!("{:.1}%", a * 100.0));
    let mut ctx = format!("🤖 ML MODEL PREDICTIONS (accuracy: {}):\n", acc_str);

    for pred in predictions.iter().take(10) {
        let emoji = if pred.ml_win_probability >= 0.6 {
            "✅"
        } else if pred.ml_win_probability >= 0.45 {
            "⚠️"
        } else {
            "❌"
        };
        ctx.push_str(&format!(
            "  {} {} {} {} (cat:{}) — ML Win Prob: {:.1}% ({}), Line: {:.1}\n",
            emoji,
            pred.player_name,
            pred.ml_prediction,
            pred.stat_category,
            pred.category_code.unwrap_or(0),
            pred.ml_win_probability * 100.0,
            if pred.ml_win_probability >= 0.5 {
                "Lean Over"
            } else {
                "Lean Under"
            },
            pred.line
        ));
    }

    ctx.push('\n');
    ctx
}

/// Export features as CSV for external analysis
pub async fn export_features_csv(
    output_path: Option<&str>,
) -> Result<String, String> {
    let db = default_db_path();
    let output = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home)
                .join(".openclaw/kalshi-monster/ml_features.csv")
        });
    let script = ml_script_path();

    let output_str = output.display().to_string();

    let result = tokio::task::spawn_blocking(move || {
        let out = Command::new("python3")
            .arg(&script)
            .arg("export-features")
            .arg("--db")
            .arg(db.display().to_string())
            .arg("--output")
            .arg(&output_str)
            .output()
            .map_err(|e| format!("Failed to run ml_predictor.py: {}", e))?;

        let stdout = String::from_utf8_lossy(&out.stdout).to_string();

        let json_line = stdout
            .lines()
            .rev()
            .find(|l| l.trim().starts_with('{'))
            .ok_or("No JSON output from ml_predictor.py")?;

        #[derive(Deserialize)]
        struct ExportResult {
            status: String,
            samples: Option<i64>,
            output_path: String,
        }

        let r: ExportResult = serde_json::from_str(json_line)
            .map_err(|e| format!("Failed to parse export output: {}", e))?;

        Ok::<_, String>(r)
    })
    .await
    .map_err(|e| format!("Task join error: {}", e))??;

    Ok(result.output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ml_prediction_deserializes_category_code() {
        let json = r#"{
            "prediction_id": "p1",
            "player_name": "FED-25DEC",
            "stat_category": "Economics",
            "line": 62.0,
            "ml_win_probability": 0.55,
            "ml_prediction": "Win",
            "original_confidence": 55,
            "original_probability": 0.55,
            "line_change": 0.0,
            "category_code": 2
        }"#;
        let pred: MLPrediction = serde_json::from_str(json).expect("parse");
        assert_eq!(pred.category_code, Some(2));
    }

    #[test]
    fn format_category_readiness_lists_trainable_flags() {
        let stats = vec![
            MLCategoryStats {
                category: "Politics".into(),
                resolved_count: 12,
                pending_count: 1,
                trainable: true,
                samples_until_trainable: 0,
                min_resolved_for_sidecar: 10,
            },
            MLCategoryStats {
                category: "Weather".into(),
                resolved_count: 3,
                pending_count: 0,
                trainable: false,
                samples_until_trainable: 7,
                min_resolved_for_sidecar: 10,
            },
        ];
        let msg = format_category_readiness(&stats);
        assert!(msg.contains("Politics: ready"));
        assert!(msg.contains("Weather: 3/10 graded (7 more for sidecar)"));
    }

    #[test]
    fn should_retrain_after_grading_requires_positive_count() {
        assert!(!should_retrain_after_grading(0));
        assert!(should_retrain_after_grading(1));
    }

    #[test]
    fn should_retrain_given_resolved_requires_ten_graded_rows() {
        assert!(!should_retrain_given_resolved(1, 9));
        assert!(should_retrain_given_resolved(1, 10));
        assert!(!should_retrain_given_resolved(0, 100));
    }

    #[test]
    fn count_trainable_non_sports_ignores_sports_and_subthreshold() {
        let stats = vec![
            MLCategoryStats {
                category: "Politics".into(),
                resolved_count: 11,
                pending_count: 0,
                trainable: true,
                samples_until_trainable: 0,
                min_resolved_for_sidecar: 10,
            },
            MLCategoryStats {
                category: "Economics".into(),
                resolved_count: 10,
                pending_count: 0,
                trainable: true,
                samples_until_trainable: 0,
                min_resolved_for_sidecar: 10,
            },
            MLCategoryStats {
                category: "Weather".into(),
                resolved_count: 4,
                pending_count: 0,
                trainable: false,
                samples_until_trainable: 6,
                min_resolved_for_sidecar: 10,
            },
            MLCategoryStats {
                category: "Sports".into(),
                resolved_count: 50,
                pending_count: 0,
                trainable: true,
                samples_until_trainable: 0,
                min_resolved_for_sidecar: 10,
            },
        ];
        assert_eq!(count_trainable_non_sports_categories(&stats), 2);
    }

    #[test]
    fn ensure_non_sports_sidecar_stats_adds_missing_targets() {
        let stats = vec![MLCategoryStats {
            category: "Sports".into(),
            resolved_count: 20,
            pending_count: 0,
            trainable: true,
            samples_until_trainable: 0,
            min_resolved_for_sidecar: 10,
        }];
        let merged = ensure_non_sports_sidecar_stats(stats);
        assert!(merged.iter().any(|s| s.category == "Politics"));
        assert!(merged.iter().any(|s| s.category == "Economics"));
        assert!(merged.iter().any(|s| s.category == "Weather"));
        let politics = merged
            .iter()
            .find(|s| s.category == "Politics")
            .expect("politics");
        assert_eq!(politics.resolved_count, 0);
        assert_eq!(politics.samples_until_trainable, 10);
    }

    #[test]
    fn nearest_non_sports_sidecar_unlock_picks_smallest_gap() {
        let stats = ensure_non_sports_sidecar_stats(vec![
            MLCategoryStats {
                category: "Politics".into(),
                resolved_count: 8,
                pending_count: 0,
                trainable: false,
                samples_until_trainable: 2,
                min_resolved_for_sidecar: 10,
            },
            MLCategoryStats {
                category: "Weather".into(),
                resolved_count: 3,
                pending_count: 0,
                trainable: false,
                samples_until_trainable: 7,
                min_resolved_for_sidecar: 10,
            },
        ]);
        let nearest = nearest_non_sports_sidecar_unlock(&stats).expect("hint");
        assert_eq!(nearest.0, "Politics");
        assert_eq!(nearest.1, 2);
    }

    #[test]
    fn auto_retrain_threshold_helpers() {
        assert!(!auto_retrain_eligible(9));
        assert!(auto_retrain_eligible(10));
        assert_eq!(resolved_until_auto_retrain(9), 1);
        assert_eq!(resolved_until_auto_retrain(10), 0);
    }

    #[test]
    fn format_ml_training_header_includes_sidecars_and_cv_std() {
        let mut sidecars = std::collections::HashMap::new();
        sidecars.insert(
            "Politics".to_string(),
            MLPerCategoryModel {
                model_exists: true,
                samples: 12,
                cv_accuracy_mean: Some(0.62),
            },
        );
        let status = MLModelStatus {
            model_exists: true,
            model_path: "/tmp/model.joblib".into(),
            trained_at: None,
            samples: Some(40),
            cv_accuracy_mean: Some(0.55),
            cv_accuracy_std: Some(0.04),
            win_rate: None,
            feature_importance: None,
            pending_predictions: 0,
            resolved_predictions: 40,
            category_stats: vec![],
            training_category_breakdown: None,
            per_category_models: Some(sidecars),
            trainable_non_sports_categories: 1,
            non_sports_sidecar_target: 3,
            next_sidecar_category: None,
            next_sidecar_samples_needed: None,
            auto_retrain_eligible: true,
            resolved_until_auto_retrain: 0,
            message: String::new(),
        };
        let header = format_ml_training_header(&status);
        assert!(header.contains("40 samples"));
        assert!(header.contains("55.0% ± 4.0%"));
        assert!(header.contains("active sidecars: Politics"));
    }
}
