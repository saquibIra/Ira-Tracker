"""Ira service: AI auto-tracking support for the Ira Tracker widget.

Run: uv run --env-file .env uvicorn --factory ira_service.main:create_app --port 8010 --no-access-log
"""

from __future__ import annotations

import hashlib
import json
import logging
import re
import time
import uuid
from contextlib import asynccontextmanager
from datetime import UTC, date, datetime, timedelta
from pathlib import Path

from fastapi import Depends, FastAPI, HTTPException, Request, Response
from fastapi.responses import FileResponse, JSONResponse, RedirectResponse
from pydantic import BaseModel, Field, field_validator
from starlette.datastructures import UploadFile

from . import logs
from .access import Auth, Caller, Catalogs, NotMember, RateLimiter
from .classify import AiAccountError, AiUnavailable, Catalog, Classifier, Project, make_backend
from .config import Settings
from .solidtime import Solidtime, SolidtimeError, Unauthorized, Unavailable
from .store import Store

log = logging.getLogger("ira")

UUID_RE = re.compile(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
DAY_RE = re.compile(r"\d{4}-\d{2}-\d{2}")
REQUEST_ID_RE = re.compile(r"[A-Za-z0-9._-]{8,64}")
STATIC = Path(__file__).parent / "static"
# Multipart framing around the image and two short text fields.
FORM_OVERHEAD = 16_000
# After an account-level AI error (no credit, bad key/permissions), stop calling for this long.
AI_PAUSE_SECS = 600

ADMIN_CSP = (
    "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; "
    "img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
)


class HintIn(BaseModel):
    description: str = Field(default="", max_length=1000)
    keywords: list[str] = Field(default_factory=list, max_length=30)

    @field_validator("description")
    @classmethod
    def one_line(cls, v: str) -> str:
        return re.sub(r"\s+", " ", v).strip()

    @field_validator("keywords")
    @classmethod
    def clean_keywords(cls, v: list[str]) -> list[str]:
        out: list[str] = []
        for k in v:
            k = re.sub(r"\s+", " ", k).strip()
            if len(k) > 60:
                raise ValueError("each keyword must be at most 60 characters")
            if len(k) >= 2 and k.lower() not in (x.lower() for x in out):
                out.append(k)
        return out


class SubmitIn(BaseModel):
    total_secs: int = Field(ge=0, le=86_400)
    entries: int = Field(ge=0, le=2000)


def create_app(
    settings: Settings | None = None,
    *,
    solidtime: Solidtime | None = None,
    store: Store | None = None,
    ai_client=None,
) -> FastAPI:
    settings = settings or Settings.from_env()
    logs.setup(settings.log_level, settings.log_format)
    solidtime = solidtime or Solidtime(settings.solidtime_url)
    store = store or Store(settings.db_path)
    if ai_client is not None:
        backend, why = ai_client, None
    else:
        backend, why = make_backend(settings)
    classifier = Classifier(backend, settings) if backend is not None else None
    log.info("starting", extra={"settings": settings.summary(), "ai_ready": classifier is not None})
    if classifier is None:
        log.warning("auto-tracking classification is disabled", extra={"reason": why})

    auth = Auth(solidtime, settings.auth_cache_secs)
    catalogs = Catalogs(solidtime, settings.catalog_cache_secs)
    limiter = RateLimiter(settings.rate_limit_per_hour)
    # Account-level AI problem: reason, and until when calls are paused (monotonic time).
    ai_block: dict = {"until": 0.0, "reason": None}

    def ai_error() -> str | None:
        return ai_block["reason"] if time.monotonic() < ai_block["until"] else None

    @asynccontextmanager
    async def lifespan(_: FastAPI):
        yield
        await solidtime.aclose()
        store.close()

    app = FastAPI(title="Ira service", lifespan=lifespan, docs_url=None, redoc_url=None, openapi_url=None)

    @app.middleware("http")
    async def request_log(request: Request, call_next):
        """Request id, security headers and one access-log line per request."""
        incoming = request.headers.get("x-request-id", "")
        rid = incoming if REQUEST_ID_RE.fullmatch(incoming) else uuid.uuid4().hex[:16]
        token = logs.request_id.set(rid)
        started = time.monotonic()
        status = 500
        try:
            response = await call_next(request)
            status = response.status_code
        finally:
            ms = int((time.monotonic() - started) * 1000)
            member = getattr(request.state, "member", None)
            level = logging.DEBUG if request.url.path == "/healthz" else logging.INFO
            if status >= 500:
                level = logging.ERROR
            log.log(
                level,
                "request",
                extra={
                    "http": {"method": request.method, "path": request.url.path, "status": status, "ms": ms},
                    "member": member,
                },
            )
            logs.request_id.reset(token)
        response.headers["X-Request-ID"] = rid
        response.headers["X-Content-Type-Options"] = "nosniff"
        response.headers["Referrer-Policy"] = "no-referrer"
        response.headers.setdefault("Cache-Control", "no-store")
        if request.url.path.startswith("/admin"):
            response.headers["Content-Security-Policy"] = ADMIN_CSP
            response.headers["X-Frame-Options"] = "DENY"
        return response

    async def caller(request: Request) -> Caller:
        header = request.headers.get("authorization", "")
        if not header.lower().startswith("bearer "):
            raise HTTPException(401, "Missing token")
        token = header[7:].strip()
        if not token or len(token) > 4096:
            raise HTTPException(401, "Invalid token")
        org = request.headers.get("x-ira-org", "")
        if not UUID_RE.fullmatch(org):
            raise HTTPException(400, "Missing or invalid X-Ira-Org header")
        try:
            c = await auth.caller(token, org)
        except Unauthorized:
            log.info("auth rejected by solidtime")
            raise HTTPException(401, "Token rejected by solidtime") from None
        except NotMember:
            log.warning("caller is not a member of the organization", extra={"org": org})
            raise HTTPException(403, "Not a member of this organization") from None
        except Unavailable as e:
            log.error("solidtime unavailable", extra={"error": str(e)})
            raise HTTPException(503, "solidtime unavailable") from None
        except SolidtimeError as e:
            log.error("solidtime error", extra={"error": str(e)})
            raise HTTPException(502, "solidtime error") from None
        request.state.member = c.member_id
        return c

    async def admin(c: Caller = Depends(caller)) -> Caller:
        if c.role not in settings.admin_roles:
            log.warning("admin endpoint refused", extra={"member": c.member_id, "role": c.role})
            raise HTTPException(403, "Admins only")
        return c

    async def visible_projects(c: Caller, fresh: bool = False) -> list[Project]:
        try:
            base = await catalogs.projects(c, fresh=fresh)
        except Unauthorized:
            raise HTTPException(401, "Token rejected by solidtime") from None
        except (Unavailable, SolidtimeError) as e:
            log.error("could not load projects from solidtime", extra={"error": str(e)})
            raise HTTPException(503, "solidtime unavailable") from None
        hints = store.hints(c.org_id)
        out = []
        for p in base:
            h = hints.get(p.id)
            if h:
                p = Project(
                    id=p.id,
                    name=p.name,
                    client=p.client,
                    description=h["description"],
                    keywords=tuple(h["keywords"]),
                    tasks=p.tasks,
                )
            out.append(p)
        return out

    # ---------- widget API ----------

    @app.get("/healthz")
    async def healthz():
        return {
            "ok": True,
            "ai": classifier is not None,
            "ai_error": ai_error(),
            "provider": settings.ai_provider,
        }

    @app.get("/v1/organizations")
    async def organizations(request: Request):
        """Organizations of the token's user (the admin page uses this to pick one)."""
        header = request.headers.get("authorization", "")
        token = header[7:].strip() if header.lower().startswith("bearer ") else ""
        if not token or len(token) > 4096:
            raise HTTPException(401, "Missing token")
        try:
            memberships = await solidtime.memberships(token)
        except Unauthorized:
            raise HTTPException(401, "Token rejected by solidtime") from None
        except SolidtimeError as e:
            log.error("solidtime unavailable", extra={"error": str(e)})
            raise HTTPException(503, "solidtime unavailable") from None
        return {
            "organizations": [
                {
                    "org_id": (m.get("organization") or {}).get("id"),
                    "org_name": (m.get("organization") or {}).get("name", ""),
                    "role": m.get("role", ""),
                }
                for m in memberships
            ]
        }

    @app.get("/v1/config")
    async def get_config(_: Caller = Depends(caller)):
        return settings.policy(ai_ready=classifier is not None)

    @app.get("/v1/hints")
    async def get_hints(request: Request, c: Caller = Depends(caller)):
        projects = await visible_projects(c)
        body = {
            "hints": [
                {"project_id": p.id, "keywords": list(p.keywords)}
                for p in sorted(projects, key=lambda p: p.id)
                if p.keywords
            ]
        }
        raw = json.dumps(body, separators=(",", ":"), sort_keys=True)
        etag = '"' + hashlib.sha256(raw.encode()).hexdigest()[:32] + '"'
        if request.headers.get("if-none-match") == etag:
            return Response(status_code=304, headers={"ETag": etag})
        return Response(raw, media_type="application/json", headers={"ETag": etag})

    @app.post("/v1/classify")
    async def classify(request: Request, c: Caller = Depends(caller)):
        if classifier is None:
            raise HTTPException(503, "AI classification is not configured on the server")
        if (reason := ai_error()) is not None:
            retry = str(int(ai_block["until"] - time.monotonic()) + 1)
            raise HTTPException(503, f"AI unavailable: {reason}", headers={"Retry-After": retry})
        # The image must stay in memory: reject anything that could spill to a temp file.
        length = request.headers.get("content-length")
        if length is None or not length.isdigit():
            raise HTTPException(411, "Content-Length required")
        if int(length) > settings.max_image_bytes + FORM_OVERHEAD:
            raise HTTPException(413, "Request too large")
        allowed, retry = limiter.allow(c.key)
        if not allowed:
            log.warning("rate limit reached", extra={"member": c.member_id})
            raise HTTPException(429, "Too many requests", headers={"Retry-After": str(retry)})

        form = await request.form(max_files=1, max_fields=4, max_part_size=4096)
        image: bytes | None = None
        try:
            app_name = str(form.get("app") or "")[:200]
            title = str(form.get("title") or "")[:1000]
            upload = form.get("image")
            if isinstance(upload, UploadFile):
                image = await upload.read(settings.max_image_bytes + 1)
                if len(image) > settings.max_image_bytes:
                    raise HTTPException(413, "Image too large")
                if not image.startswith(b"\xff\xd8\xff"):
                    raise HTTPException(415, "Image must be JPEG")
        finally:
            await form.close()

        catalog = Catalog.build(await visible_projects(c))
        has_image = image is not None
        image_kb = len(image) // 1024 if image else 0
        try:
            result = await classifier.classify(catalog, app_name, title, image)
        except AiUnavailable as e:
            log.warning(
                "AI temporarily unavailable", extra={"error": str(e), "provider": settings.ai_provider}
            )
            raise HTTPException(503, "AI temporarily unavailable", headers={"Retry-After": "60"}) from None
        except AiAccountError as e:
            # Billing, key or permission problem: every further call would fail the same way.
            ai_block.update(until=time.monotonic() + AI_PAUSE_SECS, reason=str(e))
            log.error(
                "AI account problem, pausing AI calls",
                extra={"error": str(e), "pause_secs": AI_PAUSE_SECS, "provider": settings.ai_provider},
            )
            raise HTTPException(
                503, f"AI unavailable: {e}", headers={"Retry-After": str(AI_PAUSE_SECS)}
            ) from None
        except Exception:
            log.exception("classification failed", extra={"provider": settings.ai_provider})
            raise HTTPException(502, "Classification failed") from None
        finally:
            image = None  # drop the screenshot as soon as the AI has seen it

        for call in result.calls:
            store.log_usage(
                c.org_id,
                c.member_id,
                call["model"],
                call["usage"],
                call["latency_ms"],
                call.get("outcome", "ok"),
                has_image=has_image,
            )
        # Never log the window title or the image; the app name is enough to debug.
        log.info(
            "classified",
            extra={
                "member": c.member_id,
                "app": app_name[:60],
                "has_image": has_image,
                "image_kb": image_kb,
                "projects_offered": len(catalog.refs),
                "result": {
                    "project_id": result.project_id,
                    "category": result.category,
                    "confidence": round(result.confidence, 3),
                    "model": result.model,
                },
                "calls": [
                    {
                        "model": x["model"],
                        "outcome": x.get("outcome"),
                        "ms": x["latency_ms"],
                        "in": x["usage"]["input_tokens"],
                        "out": x["usage"]["output_tokens"],
                    }
                    for x in result.calls
                ],
            },
        )
        return result.public()

    @app.post("/v1/days/{day}/submit")
    async def submit_day(day: str, body: SubmitIn, c: Caller = Depends(caller)):
        if not DAY_RE.fullmatch(day):
            raise HTTPException(400, "Day must be YYYY-MM-DD")
        try:
            d = date.fromisoformat(day)
        except ValueError:
            raise HTTPException(400, "Invalid day") from None
        today = datetime.now(UTC).date()
        if d > today + timedelta(days=1) or d < today - timedelta(days=62):
            raise HTTPException(400, "Day out of range")
        store.submit_day(c.org_id, c.member_id, day, body.total_secs, body.entries)
        log.info("day submitted", extra={"member": c.member_id, "day": day, "total_secs": body.total_secs})
        return {"ok": True}

    # ---------- admin page (works at / and behind a path prefix such as /ira) ----------

    @app.get("/admin")
    async def admin_redirect():
        # Relative redirect so it also works behind the /ira prefix.
        return RedirectResponse("admin/", status_code=307)

    @app.get("/admin/")
    async def admin_page():
        return FileResponse(STATIC / "admin.html", media_type="text/html")

    @app.get("/admin/{name}")
    async def admin_asset(name: str):
        types = {"admin.js": "text/javascript", "admin.css": "text/css"}
        if name not in types:
            raise HTTPException(404)
        return FileResponse(STATIC / name, media_type=types[name])

    @app.get("/v1/admin/projects")
    async def admin_projects(c: Caller = Depends(admin)):
        projects = await visible_projects(c, fresh=True)
        return {
            "projects": [
                {
                    "id": p.id,
                    "name": p.name,
                    "client": p.client,
                    "description": p.description,
                    "keywords": list(p.keywords),
                }
                for p in sorted(projects, key=lambda p: ((p.client or "").lower(), p.name.lower()))
            ]
        }

    @app.put("/v1/admin/hints/{project_id}")
    async def admin_set_hint(project_id: str, body: HintIn, c: Caller = Depends(admin)):
        if not UUID_RE.fullmatch(project_id):
            raise HTTPException(400, "Invalid project id")
        if not any(p.id == project_id for p in await visible_projects(c, fresh=True)):
            raise HTTPException(404, "Unknown project")
        store.set_hint(c.org_id, project_id, body.description, body.keywords, c.member_id)
        log.info(
            "project hints updated",
            extra={"member": c.member_id, "project_id": project_id, "keywords": len(body.keywords)},
        )
        return {"ok": True}

    @app.get("/v1/admin/usage")
    async def admin_usage(days: int = 30, c: Caller = Depends(admin)):
        return {**store.usage_summary(c.org_id, max(1, min(days, 366))), "ai_error": ai_error()}

    @app.get("/v1/admin/submissions")
    async def admin_submissions(day: str, c: Caller = Depends(admin)):
        if not DAY_RE.fullmatch(day):
            raise HTTPException(400, "Day must be YYYY-MM-DD")
        try:
            members = await solidtime.members(c.token, c.org_id)
        except SolidtimeError as e:
            log.warning("could not load members from solidtime", extra={"error": str(e)})
            members = []
        submitted = {s["member_id"]: s for s in store.submissions(c.org_id, day)}
        rows = [
            {
                "member_id": m["id"],
                "name": m.get("name") or "",
                "submitted": submitted.pop(m["id"], None),
            }
            for m in members
            if m.get("role") != "placeholder"
        ]
        rows += [{"member_id": k, "name": "", "submitted": v} for k, v in submitted.items()]
        return {"day": day, "members": rows}

    @app.exception_handler(json.JSONDecodeError)
    async def bad_json(_: Request, __: json.JSONDecodeError):
        return JSONResponse({"detail": "Invalid JSON"}, status_code=400)

    return app
