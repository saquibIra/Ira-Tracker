"""Logging for debugging in production.

- JSON lines (IRA_LOG_FORMAT=json) that Google Cloud Logging parses: `severity`,
  `message`, plus structured fields; readable text lines for local runs.
- Every line written while handling a request carries its `request_id` (also returned
  to the caller in the `X-Request-ID` header, so a widget error can be matched to logs).

Never log screenshots, window titles, tokens or API keys.
"""

from __future__ import annotations

import contextvars
import json
import logging
import sys
from datetime import UTC, datetime

request_id: contextvars.ContextVar[str | None] = contextvars.ContextVar("request_id", default=None)

# Attributes every LogRecord has; anything else passed via `extra=` is a structured field.
_STANDARD = set(logging.LogRecord("", 0, "", 0, "", (), None).__dict__) | {"message", "asctime"}


def fields(record: logging.LogRecord) -> dict:
    out = {k: v for k, v in record.__dict__.items() if k not in _STANDARD and not k.startswith("_")}
    rid = request_id.get()
    if rid:
        out["request_id"] = rid
    return out


class JsonFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        entry = {
            "severity": record.levelname,
            "message": record.getMessage(),
            "time": datetime.fromtimestamp(record.created, UTC).isoformat(timespec="milliseconds"),
            "logger": record.name,
            **fields(record),
        }
        if record.exc_info:
            entry["exception"] = self.formatException(record.exc_info)
        return json.dumps(entry, default=str, ensure_ascii=False)


class TextFormatter(logging.Formatter):
    def format(self, record: logging.LogRecord) -> str:
        time = datetime.fromtimestamp(record.created).strftime("%H:%M:%S")
        extra = " ".join(f"{k}={v}" for k, v in fields(record).items())
        line = f"{time} {record.levelname:<7} {record.name}: {record.getMessage()}" + (
            f"  {extra}" if extra else ""
        )
        if record.exc_info:
            line += "\n" + self.formatException(record.exc_info)
        return line


def setup(level: str = "INFO", fmt: str = "text") -> None:
    handler = logging.StreamHandler(sys.stdout)
    handler.setFormatter(JsonFormatter() if fmt == "json" else TextFormatter())
    root = logging.getLogger("ira")
    root.handlers = [handler]
    root.setLevel(level.upper())
    root.propagate = False
