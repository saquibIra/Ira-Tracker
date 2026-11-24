"""Test doubles: a fake solidtime API (httpx2.MockTransport) and a fake Claude client."""

from __future__ import annotations

import json
from types import SimpleNamespace

import httpx2
import pytest
from fastapi.testclient import TestClient

from autotrack.config import Settings
from autotrack.main import create_app
from autotrack.solidtime import Solidtime
from autotrack.store import Store

ORG = "11111111-1111-1111-1111-111111111111"
OTHER_ORG = "99999999-9999-9999-9999-999999999999"
P_PORTAL = "aaaaaaaa-0000-0000-0000-000000000001"
P_BILLING = "aaaaaaaa-0000-0000-0000-000000000002"
P_ARCHIVED = "aaaaaaaa-0000-0000-0000-000000000003"
T_API = "bbbbbbbb-0000-0000-0000-000000000001"

ENGINEER = "engineer-token"
ADMIN = "admin-token"

MEMBERS = {
    ENGINEER: {"id": "member-eng", "role": "employee"},
    ADMIN: {"id": "member-admin", "role": "admin"},
}


class FakeSolidtime:
    def __init__(self):
        self.calls: list[str] = []
        self.down = False
        self.extra_projects: list[dict] = []

    def handler(self, request: httpx2.Request) -> httpx2.Response:
        self.calls.append(request.url.path)
        if self.down:
            return httpx2.Response(503)
        token = request.headers.get("authorization", "").removeprefix("Bearer ")
        member = MEMBERS.get(token)
        if member is None:
            return httpx2.Response(401, json={"message": "Unauthenticated."})
        path = request.url.path
        if path == "/api/v1/users/me/memberships":
            return httpx2.Response(
                200,
                json={
                    "data": [
                        {**member, "organization": {"id": ORG, "name": "Acme"}},
                        {
                            "id": "m-personal",
                            "role": "owner",
                            "organization": {"id": OTHER_ORG, "name": "Personal"},
                        },
                    ]
                },
            )
        if path == f"/api/v1/organizations/{ORG}/projects":
            return httpx2.Response(
                200,
                json={
                    "data": [
                        {"id": P_PORTAL, "name": "Acme Portal", "client_id": "c1", "is_archived": False},
                        {"id": P_BILLING, "name": "Billing", "client_id": None, "is_archived": False},
                        {"id": P_ARCHIVED, "name": "Old", "client_id": None, "is_archived": True},
                        *self.extra_projects,
                    ]
                },
            )
        if path == f"/api/v1/organizations/{ORG}/tasks":
            return httpx2.Response(
                200, json={"data": [{"id": T_API, "name": "API", "project_id": P_PORTAL, "is_done": False}]}
            )
        if path == f"/api/v1/organizations/{ORG}/clients":
            return httpx2.Response(200, json={"data": [{"id": "c1", "name": "Acme Corp"}]})
        if path == f"/api/v1/organizations/{ORG}/members":
            return httpx2.Response(
                200,
                json={
                    "data": [
                        {"id": "member-eng", "name": "Engineer One", "role": "employee"},
                        {"id": "member-admin", "name": "Admin", "role": "admin"},
                    ]
                },
            )
        return httpx2.Response(404)


class FakeClaude:
    """Mimics client.beta.messages.create; answers are queued per call."""

    def __init__(self):
        self.answers: list = []
        self.requests: list[dict] = []
        self.beta = SimpleNamespace(messages=SimpleNamespace(create=self._create))

    def answer(self, ref="P1", category="work", confidence=0.9, activity="Coding", stop="end_turn"):
        self.answers.append(
            (
                json.dumps(
                    {"ref": ref, "category": category, "confidence": confidence, "activity": activity}
                ),
                stop,
            )
        )

    async def _create(self, **kwargs):
        self.requests.append(kwargs)
        item = (
            self.answers.pop(0)
            if self.answers
            else (
                json.dumps({"ref": "none", "category": "unclear", "confidence": 0, "activity": ""}),
                "end_turn",
            )
        )
        if isinstance(item, Exception):
            raise item
        text, stop = item
        return SimpleNamespace(
            content=[SimpleNamespace(type="text", text=text)],
            stop_reason=stop,
            model=kwargs["model"],
            usage=SimpleNamespace(
                input_tokens=1500, output_tokens=60, cache_read_input_tokens=0, cache_creation_input_tokens=0
            ),
        )


def settings(tmp_path, **over) -> Settings:
    base = {"SOLIDTIME_URL": "http://solidtime.test", "TIMEPILOT_DB_PATH": str(tmp_path / "t.db")}
    base.update(over)
    return Settings.from_env(base)


@pytest.fixture
def fake_st():
    return FakeSolidtime()


@pytest.fixture
def claude():
    return FakeClaude()


@pytest.fixture
def make_client(tmp_path, fake_st, claude):
    def make(ai=True, **env):
        s = settings(tmp_path, **env)
        app = create_app(
            s,
            solidtime=Solidtime(s.solidtime_url, transport=httpx2.MockTransport(fake_st.handler)),
            store=Store(s.db_path),
            ai_client=claude if ai else None,
        )
        return TestClient(app)

    return make


@pytest.fixture
def client(make_client):
    with make_client() as c:
        yield c


def h(token=ENGINEER, org=ORG) -> dict:
    return {"Authorization": f"Bearer {token}", "X-TimePilot-Org": org}


JPEG = b"\xff\xd8\xff\xe0" + b"\x00" * 2000


def anthropic_error(cls, status: int, message: str):
    """A real anthropic SDK error, as the client would raise it."""
    import anthropic

    response = httpx2.Response(
        status, request=httpx2.Request("POST", "https://api.anthropic.com/v1/messages")
    )
    body = {"type": "error", "error": {"type": "invalid_request_error", "message": message}}
    return getattr(anthropic, cls)(message=f"Error code: {status}", response=response, body=body)


CREDIT_MSG = "Your credit balance is too low to access the Anthropic API."
