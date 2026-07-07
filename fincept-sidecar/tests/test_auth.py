# SPDX-License-Identifier: AGPL-3.0-or-later
"""Every route requires the per-launch bearer token (plan §10.2)."""

from fastapi.testclient import TestClient

from fincept_sidecar.app import create_app

TOKEN = "test-token-123"


def client() -> TestClient:
    return TestClient(create_app(TOKEN))


def test_missing_token_is_401():
    r = client().get("/api/v1/health")
    assert r.status_code == 401


def test_wrong_token_is_401():
    r = client().get("/api/v1/health", headers={"Authorization": "Bearer nope"})
    assert r.status_code == 401


def test_malformed_header_is_401():
    r = client().get("/api/v1/health", headers={"Authorization": TOKEN})  # no "Bearer "
    assert r.status_code == 401


def test_correct_token_reaches_health():
    r = client().get("/api/v1/health", headers={"Authorization": f"Bearer {TOKEN}"})
    assert r.status_code == 200
    body = r.json()
    assert body["status"] == "ok"
    assert body["uptime_seconds"] >= 0


def test_version_reports_git_sha(monkeypatch):
    monkeypatch.setenv("FINCEPT_GIT_SHA", "abc1234")
    r = client().get("/api/v1/version", headers={"Authorization": f"Bearer {TOKEN}"})
    assert r.status_code == 200
    assert r.json()["git_sha"] == "abc1234"


def test_empty_token_refused_at_construction():
    import pytest

    with pytest.raises(ValueError):
        create_app("")
