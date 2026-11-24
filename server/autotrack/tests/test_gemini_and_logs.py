import asyncio
import json
import logging
from types import SimpleNamespace

import pytest
from google.genai import errors

from autotrack import logs
from autotrack.classify import AiAccountError, AiUnavailable, Catalog, Classifier, GeminiBackend, Project
from autotrack.config import ConfigError, Settings

from .conftest import JPEG, h

PROJECTS = [Project(id="b", name="Billing", client=None), Project(id="a", name="Portal", client="Acme")]


class FakeGemini:
    """Mimics client.aio.models.generate_content of the google-genai SDK."""

    def __init__(self):
        self.answers: list = []
        self.requests: list[dict] = []
        self.aio = SimpleNamespace(models=SimpleNamespace(generate_content=self._generate))

    def answer(
        self, ref="P1", category="work", confidence=0.9, activity="Coding", finish="STOP", blocked=None
    ):
        text = json.dumps({"ref": ref, "category": category, "confidence": confidence, "activity": activity})
        self.answers.append((text, finish, blocked))

    async def _generate(self, *, model, contents, config):
        self.requests.append({"model": model, "contents": contents, "config": config})
        item = self.answers.pop(0)
        if isinstance(item, Exception):
            raise item
        text, finish, blocked = item
        return SimpleNamespace(
            text=text,
            candidates=[SimpleNamespace(finish_reason=finish)],
            prompt_feedback=SimpleNamespace(block_reason=blocked) if blocked else None,
            usage_metadata=SimpleNamespace(
                prompt_token_count=1400,
                candidates_token_count=40,
                thoughts_token_count=60,
                cached_content_token_count=0,
            ),
        )


def gemini_settings(tmp_path, **over):
    env = {
        "SOLIDTIME_URL": "http://solidtime.test",
        "TIMEPILOT_DB_PATH": str(tmp_path / "t.db"),
        "TIMEPILOT_AI_PROVIDER": "gemini",
        "GOOGLE_CLOUD_PROJECT": "timepilot-test",
    }
    env.update(over)
    return Settings.from_env(env)


def run(coro):
    return asyncio.run(coro)


def test_gemini_defaults_are_flash_lite_then_flash(tmp_path):
    s = gemini_settings(tmp_path)
    assert (s.ai_model, s.ai_escalation_model) == ("gemini-3.5-flash-lite", "gemini-3.8-flash")
    assert s.gemini_location == "global" and s.gcp_project == "timepilot-test"
    with pytest.raises(ConfigError):
        gemini_settings(tmp_path, TIMEPILOT_AI_PROVIDER="openai")


def test_gemini_answer_is_validated_and_usage_counts_thinking(tmp_path):
    g = FakeGemini()
    g.answer(ref="P2", confidence=0.93, activity="Fixing portal login")
    c = Classifier(GeminiBackend(g, gemini_settings(tmp_path)), gemini_settings(tmp_path))
    r = run(c.classify(Catalog.build(PROJECTS), "Code", "portal — login.ts", JPEG))
    assert (r.project_id, r.model, r.activity) == ("a", "gemini-3.5-flash-lite", "Fixing portal login")
    req = g.requests[0]
    assert req["config"].response_mime_type == "application/json"
    assert req["config"].response_json_schema["required"] == ["ref", "category", "confidence", "activity"]
    parts = req["contents"][0].parts
    assert parts[0].inline_data.mime_type == "image/jpeg" and "Window title" in parts[1].text
    assert r.calls[0]["usage"] == {
        "input_tokens": 1400,
        "output_tokens": 100,
        "cache_read_tokens": 0,
        "cache_write_tokens": 0,
    }


def test_gemini_unsure_answer_escalates(tmp_path):
    g = FakeGemini()
    g.answer(ref="P1", confidence=0.3)
    g.answer(ref="P2", confidence=0.8)
    s = gemini_settings(tmp_path)
    r = run(Classifier(GeminiBackend(g, s), s).classify(Catalog.build(PROJECTS), "x", "y", None))
    assert [q["model"] for q in g.requests] == ["gemini-3.5-flash-lite", "gemini-3.8-flash"]
    assert r.project_id == "a" and len(r.calls) == 2


def test_gemini_safety_block_and_bad_reference_become_unclear(tmp_path):
    g = FakeGemini()
    g.answer(blocked="SAFETY")
    g.answer(ref="P99", confidence=1.0)
    s = gemini_settings(tmp_path, TIMEPILOT_AI_ESCALATION_MODEL="")
    b = GeminiBackend(g, s)
    r1 = run(b.ask("m", Catalog.build(PROJECTS), "x", "y", None))
    r2 = run(b.ask("m", Catalog.build(PROJECTS), "x", "y", None))
    assert (r1.calls[0]["outcome"], r1.project_id) == ("refusal", None)
    assert (r2.project_id, r2.confidence) == (None, 0.0)


@pytest.mark.parametrize(
    ("code", "expected"),
    [(429, AiUnavailable), (503, AiUnavailable), (403, AiAccountError), (404, AiAccountError)],
)
def test_gemini_errors_are_classified(tmp_path, code, expected):
    g = FakeGemini()
    g.answers.append(errors.APIError(code, {"error": {"message": "nope", "status": "X"}}))
    s = gemini_settings(tmp_path)
    with pytest.raises(expected):
        run(GeminiBackend(g, s).ask("m", Catalog.build(PROJECTS), "x", "y", None))


def test_json_logs_carry_request_id_and_fields():
    rec = logging.LogRecord("autotrack", logging.INFO, __file__, 1, "classified", (), None)
    rec.member = "m-1"
    token = logs.request_id.set("abc123def456")
    try:
        line = json.loads(logs.JsonFormatter().format(rec))
    finally:
        logs.request_id.reset(token)
    assert line["severity"] == "INFO" and line["message"] == "classified"
    assert line["member"] == "m-1" and line["request_id"] == "abc123def456"


def test_requests_get_an_id_and_logs_never_contain_titles(client, claude, caplog):
    claude.answer(ref="P1", confidence=0.95)
    caplog.set_level(logging.INFO, logger="autotrack")
    logging.getLogger("autotrack").propagate = True  # let caplog see the service's logger
    r = client.post(
        "/v1/classify",
        headers={**h(), "X-Request-ID": "widget-req-0001"},
        data={"app": "Code", "title": "secret-client-roadmap.docx"},
        files={"image": ("s.jpg", JPEG, "image/jpeg")},
    )
    assert r.status_code == 200
    assert r.headers["X-Request-ID"] == "widget-req-0001"
    assert client.get("/healthz").headers["X-Request-ID"]
    text = caplog.text + " ".join(json.dumps(getattr(x, "result", "")) for x in caplog.records)
    assert "secret-client-roadmap" not in text
    assert "engineer-token" not in text
    classified = [x for x in caplog.records if x.getMessage() == "classified"]
    assert classified and classified[0].has_image is True and classified[0].app == "Code"
