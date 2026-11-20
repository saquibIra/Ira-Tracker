"""Settings, read once from environment variables (see .env.example)."""

from __future__ import annotations

import os
import re
from collections.abc import Mapping
from dataclasses import dataclass, field

EFFORTS = ("minimal", "low", "medium", "high", "xhigh", "max")
PROVIDERS = ("anthropic", "gemini")
# Cheap model first, stronger model only for unsure answers.
DEFAULT_MODELS = {
    "anthropic": ("claude-haiku-5-5", "claude-opus-5-5"),
    "gemini": ("gemini-3.5-flash-lite", "gemini-3.8-flash"),
}

# Apps whose screen is never captured or sent anywhere. The widget treats them as
# private time. Matched case-insensitively against the app name.
DEFAULT_EXCLUDED_APPS = (
    "1Password",
    "Bitwarden",
    "Dashlane",
    "KeePass",
    "Keychain Access",
    "LastPass",
    "Passwords",
    "Messages",
    "Signal",
    "Telegram",
    "WhatsApp",
)


class ConfigError(ValueError):
    pass


def _float(env: Mapping[str, str], name: str, default: float, lo: float, hi: float) -> float:
    raw = env.get(name, "").strip()
    try:
        v = float(raw) if raw else default
    except ValueError as e:
        raise ConfigError(f"{name} must be a number") from e
    if not lo <= v <= hi:
        raise ConfigError(f"{name} must be between {lo} and {hi}")
    return v


def _int(env: Mapping[str, str], name: str, default: int, lo: int, hi: int) -> int:
    raw = env.get(name, "").strip()
    try:
        v = int(raw) if raw else default
    except ValueError as e:
        raise ConfigError(f"{name} must be a whole number") from e
    if not lo <= v <= hi:
        raise ConfigError(f"{name} must be between {lo} and {hi}")
    return v


def _hhmm(env: Mapping[str, str], name: str, default: str) -> str:
    v = env.get(name, "").strip() or default
    if not re.fullmatch(r"([01]\d|2[0-3]):[0-5]\d", v):
        raise ConfigError(f"{name} must be HH:MM (24 h)")
    return v


def _bool(env: Mapping[str, str], name: str, default: bool) -> bool:
    raw = env.get(name, "").strip().lower()
    if not raw:
        return default
    if raw in ("1", "true", "yes", "on"):
        return True
    if raw in ("0", "false", "no", "off"):
        return False
    raise ConfigError(f"{name} must be true or false")


@dataclass(frozen=True)
class Settings:
    solidtime_url: str
    db_path: str = "ira-service.db"

    # Claude: the primary model classifies every capture; the escalation model
    # re-checks only results below `ai_escalate_below` ("" turns escalation off).
    # "anthropic" (Claude, ANTHROPIC_API_KEY) or "gemini" (Vertex AI with the VM's service
    # account and GOOGLE_CLOUD_PROJECT, or GEMINI_API_KEY).
    ai_provider: str = "anthropic"
    gcp_project: str = ""
    gemini_location: str = "global"
    ai_model: str = "claude-haiku-5-5"
    ai_escalation_model: str = "claude-opus-5-5"
    ai_escalate_below: float = 0.7
    ai_effort: str = "low"
    ai_timeout_secs: float = 20.0
    ai_refusal_fallback: bool = True

    # Policy sent to every widget (GET /v1/config).
    review_below: float = 0.6
    capture_interval_secs: int = 300
    min_switch_secs: int = 120
    pin_secs: int = 1800
    unresolved_review_secs: int = 900
    capture_max_px: int = 1568
    capture_quality: int = 60
    work_days: tuple[int, ...] = (1, 2, 3, 4, 5)
    work_start: str = "09:00"
    work_end: str = "19:00"
    review_at: str = "18:30"
    excluded_apps: tuple[str, ...] = DEFAULT_EXCLUDED_APPS
    # Screenshots kept on each engineer's own computer for the day review (0 = none).
    # The service itself never stores screenshots.
    keep_screenshots_days: int = 3

    # Protection.
    rate_limit_per_hour: int = 40
    max_image_bytes: int = 900_000
    auth_cache_secs: int = 600
    catalog_cache_secs: int = 600
    admin_roles: tuple[str, ...] = field(default=("owner", "admin"))

    # Logs: level and format ("json" for Google Cloud Logging, "text" locally).
    log_level: str = "INFO"
    log_format: str = "text"

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> Settings:
        env = os.environ if env is None else env
        url = env.get("SOLIDTIME_URL", "").strip().rstrip("/")
        if not re.fullmatch(r"https?://[^\s/?#]+(/[^\s?#]*)?", url):
            raise ConfigError("SOLIDTIME_URL must be an http(s) URL, e.g. http://localhost:8000")

        effort = env.get("IRA_AI_EFFORT", "").strip() or "low"
        if effort not in EFFORTS:
            raise ConfigError(f"IRA_AI_EFFORT must be one of {', '.join(EFFORTS)}")

        provider = env.get("IRA_AI_PROVIDER", "").strip().lower() or cls.ai_provider
        if provider not in PROVIDERS:
            raise ConfigError(f"IRA_AI_PROVIDER must be one of {', '.join(PROVIDERS)}")
        default_model, default_escalation = DEFAULT_MODELS[provider]
        model = env.get("IRA_AI_MODEL", "").strip() or default_model
        escalation = env.get("IRA_AI_ESCALATION_MODEL")
        escalation = default_escalation if escalation is None else escalation.strip()

        log_level = env.get("IRA_LOG_LEVEL", "").strip().upper() or cls.log_level
        if log_level not in ("DEBUG", "INFO", "WARNING", "ERROR"):
            raise ConfigError("IRA_LOG_LEVEL must be DEBUG, INFO, WARNING or ERROR")
        log_format = env.get("IRA_LOG_FORMAT", "").strip().lower() or cls.log_format
        if log_format not in ("text", "json"):
            raise ConfigError("IRA_LOG_FORMAT must be text or json")
        location = env.get("IRA_GEMINI_LOCATION", "").strip() or cls.gemini_location
        if not re.fullmatch(r"[a-z0-9-]{2,40}", location):
            raise ConfigError("IRA_GEMINI_LOCATION must be a region like global or asia-south1")
        for name, value in (("IRA_AI_MODEL", model), ("IRA_AI_ESCALATION_MODEL", escalation)):
            if value and not re.fullmatch(r"[a-z0-9][a-z0-9.\-]{2,63}", value):
                raise ConfigError(f"{name} is not a valid model id")

        days_raw = env.get("IRA_WORK_DAYS", "").strip() or "1,2,3,4,5"
        try:
            days = tuple(sorted({int(d) for d in days_raw.split(",") if d.strip()}))
        except ValueError as e:
            raise ConfigError("IRA_WORK_DAYS must be a list like 1,2,3,4,5 (1 = Monday)") from e
        if not days or any(d < 1 or d > 7 for d in days):
            raise ConfigError("IRA_WORK_DAYS must contain days 1 (Monday) to 7 (Sunday)")

        excluded_raw = env.get("IRA_EXCLUDED_APPS")
        excluded = (
            DEFAULT_EXCLUDED_APPS
            if excluded_raw is None
            else tuple(a.strip() for a in excluded_raw.split(",") if a.strip())
        )

        work_start = _hhmm(env, "IRA_WORK_START", cls.work_start)
        work_end = _hhmm(env, "IRA_WORK_END", cls.work_end)
        if work_end <= work_start:
            raise ConfigError("IRA_WORK_END must be later than IRA_WORK_START")

        return cls(
            solidtime_url=url,
            db_path=env.get("IRA_DB_PATH", "").strip() or cls.db_path,
            ai_provider=provider,
            gcp_project=env.get("GOOGLE_CLOUD_PROJECT", "").strip(),
            gemini_location=location,
            log_level=log_level,
            log_format=log_format,
            ai_model=model,
            ai_escalation_model=escalation,
            ai_escalate_below=_float(env, "IRA_AI_ESCALATE_BELOW", cls.ai_escalate_below, 0, 1),
            ai_effort=effort,
            ai_timeout_secs=_float(env, "IRA_AI_TIMEOUT_SECS", cls.ai_timeout_secs, 2, 120),
            ai_refusal_fallback=_bool(env, "IRA_AI_REFUSAL_FALLBACK", cls.ai_refusal_fallback),
            review_below=_float(env, "IRA_REVIEW_BELOW", cls.review_below, 0, 1),
            capture_interval_secs=_int(env, "IRA_CAPTURE_INTERVAL_SECS", cls.capture_interval_secs, 30, 3600),
            min_switch_secs=_int(env, "IRA_MIN_SWITCH_SECS", cls.min_switch_secs, 0, 1800),
            pin_secs=_int(env, "IRA_PIN_SECS", cls.pin_secs, 0, 8 * 3600),
            unresolved_review_secs=_int(
                env, "IRA_UNRESOLVED_REVIEW_SECS", cls.unresolved_review_secs, 60, 8 * 3600
            ),
            capture_max_px=_int(env, "IRA_CAPTURE_MAX_PX", cls.capture_max_px, 640, 2576),
            capture_quality=_int(env, "IRA_CAPTURE_QUALITY", cls.capture_quality, 20, 95),
            work_days=days,
            work_start=work_start,
            work_end=work_end,
            review_at=_hhmm(env, "IRA_REVIEW_AT", cls.review_at),
            excluded_apps=excluded,
            keep_screenshots_days=_int(env, "IRA_KEEP_SCREENSHOTS_DAYS", cls.keep_screenshots_days, 0, 14),
            rate_limit_per_hour=_int(env, "IRA_RATE_LIMIT_PER_HOUR", cls.rate_limit_per_hour, 1, 3600),
            max_image_bytes=_int(env, "IRA_MAX_IMAGE_BYTES", cls.max_image_bytes, 10_000, 900_000),
        )

    def summary(self) -> dict:
        """Non-secret settings, logged at start-up."""
        return {
            "ai_provider": self.ai_provider,
            "ai_model": self.ai_model,
            "ai_escalation_model": self.ai_escalation_model or None,
            "ai_effort": self.ai_effort,
            "gcp_project": self.gcp_project or None,
            "gemini_location": self.gemini_location if self.ai_provider == "gemini" else None,
            "capture_interval_secs": self.capture_interval_secs,
            "keep_screenshots_days": self.keep_screenshots_days,
            "work": f"{self.work_start}-{self.work_end} days {','.join(map(str, self.work_days))}",
            "solidtime_url": self.solidtime_url,
        }

    def policy(self, ai_ready: bool) -> dict:
        """What every widget needs to know; served by GET /v1/config."""
        return {
            "enabled": ai_ready,
            "capture_interval_secs": self.capture_interval_secs,
            "min_switch_secs": self.min_switch_secs,
            "pin_secs": self.pin_secs,
            "review_below": self.review_below,
            "unresolved_review_secs": self.unresolved_review_secs,
            "capture_max_px": self.capture_max_px,
            "capture_quality": self.capture_quality,
            "work_days": list(self.work_days),
            "work_start": self.work_start,
            "work_end": self.work_end,
            "review_at": self.review_at,
            "excluded_apps": list(self.excluded_apps),
            "keep_screenshots_days": self.keep_screenshots_days,
        }
