import pytest

from ira_service.access import RateLimiter, TtlCache
from ira_service.config import ConfigError, Settings

BASE = {"SOLIDTIME_URL": "http://localhost:8000"}


def test_defaults_are_cheap_and_configurable():
    s = Settings.from_env(BASE)
    assert s.ai_model == "claude-haiku-5-5"
    assert s.ai_escalation_model == "claude-opus-5-5"
    assert s.capture_interval_secs == 300
    assert s.keep_screenshots_days == 3
    assert Settings.from_env({**BASE, "IRA_KEEP_SCREENSHOTS_DAYS": "0"}).keep_screenshots_days == 0
    s = Settings.from_env({**BASE, "IRA_AI_MODEL": "claude-opus-5-5", "IRA_AI_ESCALATION_MODEL": ""})
    assert (s.ai_model, s.ai_escalation_model) == ("claude-opus-5-5", "")


@pytest.mark.parametrize(
    "env",
    [
        {},
        {"SOLIDTIME_URL": "ftp://x"},
        {**BASE, "IRA_AI_EFFORT": "huge"},
        {**BASE, "IRA_AI_ESCALATE_BELOW": "2"},
        {**BASE, "IRA_CAPTURE_INTERVAL_SECS": "5"},
        {**BASE, "IRA_WORK_START": "25:00"},
        {**BASE, "IRA_WORK_START": "18:00", "IRA_WORK_END": "09:00"},
        {**BASE, "IRA_WORK_DAYS": "0,8"},
        {**BASE, "IRA_AI_MODEL": "Robert'); DROP TABLE"},
        {**BASE, "IRA_AI_REFUSAL_FALLBACK": "maybe"},
        {**BASE, "IRA_KEEP_SCREENSHOTS_DAYS": "30"},
    ],
)
def test_invalid_settings_fail_fast(env):
    with pytest.raises(ConfigError):
        Settings.from_env(env)


def test_excluded_apps_can_be_replaced():
    s = Settings.from_env({**BASE, "IRA_EXCLUDED_APPS": "Slack, Zoom ,"})
    assert s.excluded_apps == ("Slack", "Zoom")


def test_rate_limiter_window():
    now = [0.0]
    rl = RateLimiter(2, clock=lambda: now[0])
    assert rl.allow("a")[0] and rl.allow("a")[0]
    ok, retry = rl.allow("a")
    assert not ok and 3500 < retry <= 3601
    assert rl.allow("b")[0], "limits are per caller"
    now[0] = 3601
    assert rl.allow("a")[0]


def test_ttl_cache_expires():
    now = [0.0]
    c = TtlCache(10, clock=lambda: now[0])
    c.put("k", 1)
    assert c.get("k") == 1
    now[0] = 11
    assert c.get("k") is None
