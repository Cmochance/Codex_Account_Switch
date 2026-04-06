from __future__ import annotations

from fastapi.testclient import TestClient

from backend.api import app


def test_frontend_root_serves_control_panel():
    client = TestClient(app)

    response = client.get("/")

    assert response.status_code == 200
    assert "Account control panel" in response.text
    assert '/static/app.js' in response.text


def test_frontend_static_assets_are_served():
    client = TestClient(app)

    response = client.get("/static/app.js")

    assert response.status_code == 200
    assert "loadDashboard" in response.text
