import sqlite3
from datetime import UTC, datetime, timedelta

from .conftest import ADMIN, JPEG, OTHER_ORG, P_ARCHIVED, P_BILLING, P_PORTAL, T_API, h


def classify(client, image=JPEG, headers=None, app="Code", title="portal — api.ts"):
    files = {"image": ("s.jpg", image, "image/jpeg")} if image is not None else None
    return client.post("/v1/classify", headers=headers or h(), data={"app": app, "title": title}, files=files)


def test_health_is_public(client):
    assert client.get("/healthz").json() == {
        "ok": True,
        "ai": True,
        "ai_error": None,
        "provider": "anthropic",
    }


def test_requests_need_a_valid_token_and_org(client):
    assert client.get("/v1/config").status_code == 401
    assert (
        client.get("/v1/config", headers={"Authorization": "Bearer x", "X-Ira-Org": "nope"}).status_code
        == 400
    )
    assert client.get("/v1/config", headers=h(token="bad")).status_code == 401
    other = "22222222-2222-2222-2222-222222222222"
    assert client.get("/v1/config", headers=h(org=other)).status_code == 403


def test_config_returns_policy(client):
    c = client.get("/v1/config", headers=h()).json()
    assert c["enabled"] is True
    assert c["capture_interval_secs"] == 300
    assert c["work_days"] == [1, 2, 3, 4, 5]
    assert "1Password" in c["excluded_apps"]
    assert c["keep_screenshots_days"] == 3


def test_config_reports_disabled_without_api_key(make_client):
    with make_client(ai=False) as c:
        assert c.get("/v1/config", headers=h()).json()["enabled"] is False
        assert classify(c).status_code == 503


def test_auth_is_cached(client, fake_st):
    client.get("/v1/config", headers=h())
    client.get("/v1/config", headers=h())
    assert fake_st.calls.count("/api/v1/users/me/memberships") == 1


def test_solidtime_outage_is_503(client, fake_st):
    fake_st.down = True
    assert client.get("/v1/config", headers=h()).status_code == 503


def test_classify_returns_validated_project(client, claude):
    claude.answer(ref="P2.1", confidence=0.93, activity="Writing API endpoint")
    r = classify(client)
    assert r.status_code == 200, r.text
    body = r.json()
    assert body["project_id"] == P_PORTAL and body["task_id"] == T_API
    assert body["activity"] == "Writing API endpoint"
    prompt = claude.requests[0]["system"][1]["text"]
    assert "Acme Portal" in prompt and "Billing" in prompt
    assert "Old" not in prompt, "archived projects are never offered"


def test_classify_without_image_is_allowed(client, claude):
    claude.answer(ref="P1", confidence=0.9)
    r = classify(client, image=None, title="ira-billing")
    assert r.status_code == 200 and r.json()["project_id"] == P_BILLING


def test_classify_rejects_non_jpeg_and_large_images(client):
    assert classify(client, image=b"\x89PNG....").status_code == 415
    assert classify(client, image=b"\xff\xd8\xff" + b"0" * 950_000).status_code == 413


def test_classify_requires_content_length(client):
    r = client.post("/v1/classify", headers={**h(), "Transfer-Encoding": "chunked"}, content=iter([b"x"]))
    assert r.status_code in (411, 413)


def test_rate_limit(make_client, claude):
    with make_client(IRA_RATE_LIMIT_PER_HOUR="2") as c:
        assert classify(c).status_code == 200
        assert classify(c).status_code == 200
        r = classify(c)
        assert r.status_code == 429 and int(r.headers["Retry-After"]) > 0


def test_usage_is_logged_without_any_content(client, claude, tmp_path):
    claude.answer(ref="P1", confidence=0.95)
    classify(client, title="secret-window-title")
    db = sqlite3.connect(tmp_path / "t.db")
    rows = db.execute("SELECT * FROM usage").fetchall()
    assert len(rows) == 1
    dump = repr(rows) + repr(db.execute("SELECT * FROM hint").fetchall())
    assert "secret-window-title" not in dump
    assert "engineer-token" not in dump
    tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    assert tables == {"hint", "usage", "day_submission"}


def test_hints_flow_from_admin_to_widget(client):
    assert client.get("/v1/hints", headers=h()).json() == {"hints": []}
    r = client.put(
        f"/v1/admin/hints/{P_BILLING}",
        headers=h(token=ADMIN),
        json={
            "description": "Internal billing system",
            "keywords": ["ira-billing", " BILL- ", "ira-billing"],
        },
    )
    assert r.status_code == 200, r.text
    r = client.get("/v1/hints", headers=h())
    assert r.json() == {"hints": [{"project_id": P_BILLING, "keywords": ["ira-billing", "BILL-"]}]}
    etag = r.headers["ETag"]
    assert client.get("/v1/hints", headers={**h(), "If-None-Match": etag}).status_code == 304


def test_hint_description_reaches_the_prompt(client, claude):
    client.put(
        f"/v1/admin/hints/{P_BILLING}",
        headers=h(token=ADMIN),
        json={"description": "Invoices", "keywords": []},
    )
    claude.answer()
    classify(client)
    assert "about: Invoices" in claude.requests[0]["system"][1]["text"]


def test_admin_endpoints_require_admin_role(client):
    assert client.get("/v1/admin/projects", headers=h()).status_code == 403
    assert client.put(f"/v1/admin/hints/{P_BILLING}", headers=h(), json={}).status_code == 403
    assert client.get("/v1/admin/usage", headers=h()).status_code == 403


def test_admin_cannot_hint_unknown_or_archived_project(client):
    assert client.put(f"/v1/admin/hints/{P_ARCHIVED}", headers=h(token=ADMIN), json={}).status_code == 404
    assert client.put("/v1/admin/hints/not-a-uuid", headers=h(token=ADMIN), json={}).status_code == 400
    too_long = {"keywords": ["x" * 61]}
    assert (
        client.put(f"/v1/admin/hints/{P_BILLING}", headers=h(token=ADMIN), json=too_long).status_code == 422
    )


def test_usage_summary_estimates_cost(client, claude):
    claude.answer(confidence=0.95)
    claude.answer(confidence=0.95)
    classify(client)
    classify(client, image=None)
    u = client.get("/v1/admin/usage", headers=h(token=ADMIN)).json()
    assert u["models"][0]["model"] == "claude-haiku-5-5"
    assert u["models"][0]["calls"] == 2
    assert u["models"][0]["with_screenshot"] == 1, "title-only calls are not counted"
    assert u["estimated_cost_usd"] > 0


def test_day_submission(client):
    day = datetime.now(UTC).date().isoformat()
    r = client.post(f"/v1/days/{day}/submit", headers=h(), json={"total_secs": 28_800, "entries": 7})
    assert r.status_code == 200
    s = client.get(f"/v1/admin/submissions?day={day}", headers=h(token=ADMIN)).json()
    eng = next(m for m in s["members"] if m["member_id"] == "member-eng")
    assert eng["name"] == "Engineer One" and eng["submitted"]["total_secs"] == 28_800
    admin = next(m for m in s["members"] if m["member_id"] == "member-admin")
    assert admin["submitted"] is None


def test_day_submission_validation(client):
    far = (datetime.now(UTC).date() + timedelta(days=10)).isoformat()
    assert (
        client.post(f"/v1/days/{far}/submit", headers=h(), json={"total_secs": 1, "entries": 1}).status_code
        == 400
    )
    assert (
        client.post(
            "/v1/days/2026-13-45/submit", headers=h(), json={"total_secs": 1, "entries": 1}
        ).status_code
        == 400
    )
    day = datetime.now(UTC).date().isoformat()
    assert (
        client.post(f"/v1/days/{day}/submit", headers=h(), json={"total_secs": -1, "entries": 1}).status_code
        == 422
    )


def test_organizations_lists_memberships(client):
    orgs = client.get("/v1/organizations", headers={"Authorization": f"Bearer {ADMIN}"}).json()[
        "organizations"
    ]
    assert {o["org_id"] for o in orgs} == {"11111111-1111-1111-1111-111111111111", OTHER_ORG}


def test_admin_page_has_strict_csp(client):
    r = client.get("/admin")
    assert r.status_code == 200
    assert "script-src 'self'" in r.headers["Content-Security-Policy"]
    assert client.get("/admin/admin.js").status_code == 200
    assert client.get("/admin/../config.py").status_code == 404
    assert client.get("/admin/secret.py").status_code == 404


def test_account_problem_pauses_ai_and_is_visible(client, claude):
    from .conftest import CREDIT_MSG, anthropic_error

    claude.answers.append(anthropic_error("BadRequestError", 400, CREDIT_MSG))
    r = classify(client)
    assert r.status_code == 503
    assert "credit balance" in r.json()["detail"]
    assert int(r.headers["Retry-After"]) >= 500
    # further calls are refused without reaching Claude
    claude.answer()
    r = classify(client)
    assert r.status_code == 503 and len(claude.requests) == 1
    assert "credit balance" in client.get("/healthz").json()["ai_error"]
    usage = client.get("/v1/admin/usage", headers=h(token=ADMIN)).json()
    assert "credit balance" in usage["ai_error"]
