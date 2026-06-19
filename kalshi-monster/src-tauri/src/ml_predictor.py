#!/usr/bin/env python3
"""
PrizePicks Monster — ML Prop Prediction Trainer & Inference Engine

Trains a GradientBoosting classifier on historical prediction outcomes
combined with line movement features to predict prop win probability.

Usage:
    python3 ml_predictor.py train --db /path/to/predictions.db --output /path/to/model.joblib
    python3 ml_predictor.py predict --db /path/to/predictions.db --model /path/to/model.joblib
    python3 ml_predictor.py export-features --db /path/to/predictions.db --output /path/to/features.csv
"""

import argparse
import json
import sys
import os
import sqlite3
import numpy as np
from pathlib import Path
from datetime import datetime, timezone

# ── Feature extraction from SQLite ──

FEATURE_COLUMNS = [
    "line",
    "confidence_score",
    "probability",
    "edge_pct",
    "expected_value",
    "kelly_pct",
    "win_probability",
    "line_change",        # from line_movements: current - opening
    "line_volatility",    # stddev of line snapshots
    "snapshot_count",     # number of line snapshots for this prop
    "days_since_first",   # days since first snapshot
    "direction_up",       # 1 if line moved up
    "direction_down",     # 1 if line moved down
    "outcome_encoded",    # target: 1=Win, 0=Loss/Push
]

def extract_features_from_db(db_path: str) -> dict:
    """Extract training features from the SQLite database."""
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row

    # Get resolved predictions with outcomes
    predictions = conn.execute("""
        SELECT id, player_name, stat_category, line, confidence_score,
               probability, outcome, actual_result, created_at
        FROM predictions
        WHERE outcome IN ('Win', 'Loss', 'Push')
          AND line IS NOT NULL
        ORDER BY created_at DESC
    """).fetchall()

    if not predictions:
        conn.close()
        return {"X": np.array([]), "y": np.array([]), "metadata": []}

    # Get line movement data
    line_data = {}
    try:
        rows = conn.execute("""
            SELECT prop_key, player_name, stat_category, league,
                   MIN(line) as min_line, MAX(line) as max_line,
                   AVG(line) as avg_line,
                   COUNT(*) as snapshot_count,
                   MIN(snapshot_at) as first_seen,
                   MAX(snapshot_at) as last_updated,
                   (SELECT line FROM line_movements l2 
                    WHERE l2.prop_key = line_movements.prop_key 
                    ORDER BY snapshot_at ASC LIMIT 1) as opening_line,
                   (SELECT line FROM line_movements l3 
                    WHERE l3.prop_key = line_movements.prop_key 
                    ORDER BY snapshot_at DESC LIMIT 1) as current_line
            FROM line_movements
            GROUP BY prop_key
        """).fetchall()
        for r in rows:
            key = f"{r['player_name']}|{r['stat_category']}"
            line_data[key] = dict(r)
    except Exception:
        pass  # line_movements table may not exist

    conn.close()

    # Build feature matrix
    X_rows = []
    y_rows = []
    metadata = []

    for pred in predictions:
        player = pred["player_name"] or ""
        stat = pred["stat_category"] or ""
        key = f"{player}|{stat}"
        lm = line_data.get(key, {})

        outcome = pred["outcome"]
        target = 1 if outcome == "Win" else 0

        # Compute line features
        opening = lm.get("opening_line") or pred["line"]
        current = lm.get("current_line") or pred["line"]
        line_change = (current - opening) if opening else 0.0
        line_volatility = 0.0
        if lm.get("min_line") is not None and lm.get("max_line") is not None:
            line_volatility = lm["max_line"] - lm["min_line"]

        snap_count = lm.get("snapshot_count", 0)
        days_since_first = 0.0
        if lm.get("first_seen"):
            try:
                first = datetime.fromisoformat(lm["first_seen"].replace("Z", "+00:00"))
                days_since_first = (datetime.now(timezone.utc) - first).total_seconds() / 86400.0
            except Exception:
                pass

        # Estimate edge_pct and EV from available data
        conf = pred["confidence_score"] or 50
        edge_pct = (conf - 50) * 0.4  # rough proxy
        ev = edge_pct * 0.8
        kelly = max(0.0, ev / 10.0)
        win_prob = pred["probability"] or (50.0 + edge_pct)

        row = [
            pred["line"] or 0.0,           # line
            float(conf),                     # confidence_score
            pred["probability"] or 50.0,    # probability
            edge_pct,                        # edge_pct
            ev,                              # expected_value
            kelly,                           # kelly_pct
            win_prob,                        # win_probability
            line_change,                     # line_change
            line_volatility,                 # line_volatility
            float(snap_count),               # snapshot_count
            days_since_first,                # days_since_first
            1.0 if line_change > 0.05 else 0.0,   # direction_up
            1.0 if line_change < -0.05 else 0.0,  # direction_down
            float(target),                   # outcome_encoded (target)
        ]

        X_rows.append(row[:-1])  # all except target
        y_rows.append(row[-1])   # target
        metadata.append({
            "id": pred["id"],
            "player_name": player,
            "stat_category": stat,
            "line": pred["line"],
            "outcome": outcome,
        })

    return {
        "X": np.array(X_rows) if X_rows else np.array([]),
        "y": np.array(y_rows) if y_rows else np.array([]),
        "metadata": metadata,
    }


def train_model(db_path: str, output_path: str) -> dict:
    """Train a model on historical data."""
    from sklearn.ensemble import GradientBoostingClassifier, RandomForestClassifier
    from sklearn.model_selection import cross_val_score
    from sklearn.preprocessing import StandardScaler
    from sklearn.pipeline import Pipeline
    import joblib

    data = extract_features_from_db(db_path)
    X, y = data["X"], data["y"]

    if len(X) < 10:
        return {
            "status": "insufficient_data",
            "samples": len(X),
            "message": f"Need at least 10 resolved predictions, found {len(X)}. Resolve more predictions first.",
        }

    # GradientBoosting with StandardScaler
    pipeline = Pipeline([
        ("scaler", StandardScaler()),
        ("model", GradientBoostingClassifier(
            n_estimators=min(100, max(10, len(X) // 2)),
            max_depth=min(5, max(2, len(X) // 10)),
            learning_rate=0.1,
            random_state=42,
        )),
    ])

    # Cross-validation
    cv_folds = min(5, len(X))
    if cv_folds >= 2:
        cv_scores = cross_val_score(pipeline, X, y, cv=cv_folds, scoring="accuracy")
    else:
        cv_scores = np.array([0.0])

    # Train on full data
    pipeline.fit(X, y)

    # Feature importances
    model = pipeline.named_steps["model"]
    importances = model.feature_importances_.tolist()
    feature_importance = sorted(
        zip(FEATURE_COLUMNS, importances),
        key=lambda x: x[1],
        reverse=True,
    )

    # Save model
    Path(output_path).parent.mkdir(parents=True, exist_ok=True)
    joblib.dump(pipeline, output_path)

    # Also save feature metadata
    meta_path = output_path.replace(".joblib", "_meta.json")
    with open(meta_path, "w") as f:
        json.dump({
            "trained_at": datetime.now(timezone.utc).isoformat(),
            "samples": len(X),
            "cv_accuracy_mean": float(cv_scores.mean()),
            "cv_accuracy_std": float(cv_scores.std()),
            "feature_importance": [{"feature": ft, "importance": float(imp)} for ft, imp in feature_importance],
            "win_rate": float(y.mean()),
        }, f, indent=2)

    return {
        "status": "trained",
        "samples": len(X),
        "cv_accuracy_mean": round(float(cv_scores.mean()), 4),
        "cv_accuracy_std": round(float(cv_scores.std()), 4),
        "win_rate": round(float(y.mean()), 4),
        "model_path": output_path,
        "feature_importance": [{"feature": ft, "importance": round(float(imp), 4)} for ft, imp in feature_importance],
        "message": f"Trained on {len(X)} samples. CV accuracy: {cv_scores.mean():.1%} ± {cv_scores.std():.1%}",
    }


def predict_batch(db_path: str, model_path: str) -> dict:
    """Generate predictions for all pending props."""
    import joblib

    if not Path(model_path).exists():
        return {"status": "no_model", "message": f"Model not found at {model_path}. Train first."}

    pipeline = joblib.load(model_path)

    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row

    # Get pending predictions
    predictions = conn.execute("""
        SELECT id, player_name, stat_category, line, confidence_score,
               probability, created_at
        FROM predictions
        WHERE outcome = 'Pending'
          AND line IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 200
    """).fetchall()

    if not predictions:
        conn.close()
        return {"status": "no_pending", "predictions": []}

    # Get line movement data
    line_data = {}
    try:
        rows = conn.execute("""
            SELECT prop_key, player_name, stat_category,
                   MIN(line) as min_line, MAX(line) as max_line,
                   COUNT(*) as snapshot_count,
                   MIN(snapshot_at) as first_seen,
                   (SELECT line FROM line_movements l2 
                    WHERE l2.prop_key = line_movements.prop_key 
                    ORDER BY snapshot_at ASC LIMIT 1) as opening_line,
                   (SELECT line FROM line_movements l3 
                    WHERE l3.prop_key = line_movements.prop_key 
                    ORDER BY snapshot_at DESC LIMIT 1) as current_line
            FROM line_movements
            GROUP BY prop_key
        """).fetchall()
        for r in rows:
            key = f"{r['player_name']}|{r['stat_category']}"
            line_data[key] = dict(r)
    except Exception:
        pass

    conn.close()

    results = []
    for pred in predictions:
        player = pred["player_name"] or ""
        stat = pred["stat_category"] or ""
        key = f"{player}|{stat}"
        lm = line_data.get(key, {})

        opening = lm.get("opening_line") or pred["line"]
        current = lm.get("current_line") or pred["line"]
        line_change = (current - opening) if opening else 0.0
        line_volatility = 0.0
        if lm.get("min_line") is not None and lm.get("max_line") is not None:
            line_volatility = lm["max_line"] - lm["min_line"]

        snap_count = lm.get("snapshot_count", 0)
        days_since_first = 0.0
        if lm.get("first_seen"):
            try:
                first = datetime.fromisoformat(lm["first_seen"].replace("Z", "+00:00"))
                days_since_first = (datetime.now(timezone.utc) - first).total_seconds() / 86400.0
            except Exception:
                pass

        conf = pred["confidence_score"] or 50
        edge_pct = (conf - 50) * 0.4
        ev = edge_pct * 0.8
        kelly = max(0.0, ev / 10.0)
        win_prob = pred["probability"] or (50.0 + edge_pct)

        features = np.array([[
            pred["line"] or 0.0,
            float(conf),
            pred["probability"] or 50.0,
            edge_pct,
            ev,
            kelly,
            win_prob,
            line_change,
            line_volatility,
            float(snap_count),
            days_since_first,
            1.0 if line_change > 0.05 else 0.0,
            1.0 if line_change < -0.05 else 0.0,
        ]])

        ml_win_prob = float(pipeline.predict_proba(features)[0][1])
        ml_prediction = "Win" if ml_win_prob >= 0.5 else "Loss"

        results.append({
            "prediction_id": pred["id"],
            "player_name": player,
            "stat_category": stat,
            "line": pred["line"],
            "ml_win_probability": round(ml_win_prob, 4),
            "ml_prediction": ml_prediction,
            "original_confidence": conf,
            "original_probability": pred["probability"],
            "line_change": round(line_change, 2),
        })

    return {
        "status": "ok",
        "model_path": model_path,
        "predictions_count": len(results),
        "predictions": results,
    }


def export_features_csv(db_path: str, output_path: str) -> dict:
    """Export feature matrix as CSV for external analysis."""
    data = extract_features_from_db(db_path)
    X, y, metadata = data["X"], data["y"], data["metadata"]

    if len(X) == 0:
        return {"status": "no_data", "message": "No resolved predictions to export."}

    import csv
    Path(output_path).parent.mkdir(parents=True, exist_ok=True)

    with open(output_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(FEATURE_COLUMNS[:14] + ["outcome"])  # 14 features + target
        for i in range(len(X)):
            row = list(X[i]) + [int(y[i])]
            writer.writerow(row)

    return {
        "status": "exported",
        "samples": len(X),
        "output_path": output_path,
    }


def main():
    parser = argparse.ArgumentParser(description="PrizePicks Monster ML Engine")
    subparsers = parser.add_subparsers(dest="command", help="Command to run")

    # Train
    train_parser = subparsers.add_parser("train", help="Train ML model")
    train_parser.add_argument("--db", required=True, help="Path to SQLite database")
    train_parser.add_argument("--output", required=True, help="Output path for model.joblib")

    # Predict
    pred_parser = subparsers.add_parser("predict", help="Generate predictions")
    pred_parser.add_argument("--db", required=True, help="Path to SQLite database")
    pred_parser.add_argument("--model", required=True, help="Path to model.joblib")

    # Export features
    export_parser = subparsers.add_parser("export-features", help="Export feature CSV")
    export_parser.add_argument("--db", required=True, help="Path to SQLite database")
    export_parser.add_argument("--output", required=True, help="Output CSV path")

    # Info
    info_parser = subparsers.add_parser("info", help="Show model info")
    info_parser.add_argument("--model", required=True, help="Path to model.joblib (or _meta.json)")

    args = parser.parse_args()

    if args.command == "train":
        result = train_model(args.db, args.output)
    elif args.command == "predict":
        result = predict_batch(args.db, args.model)
    elif args.command == "export-features":
        result = export_features_csv(args.db, args.output)
    elif args.command == "info":
        meta_path = args.model.replace(".joblib", "_meta.json")
        if Path(meta_path).exists():
            with open(meta_path) as f:
                result = json.load(f)
            result["status"] = "model_info"
        else:
            result = {"status": "no_meta", "message": f"Model meta not found at {meta_path}"}
    else:
        parser.print_help()
        sys.exit(1)

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
