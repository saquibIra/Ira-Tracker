# Autotrack service

FastAPI service next to solidtime. It does three things:

1. **AI auto-tracking.** The widget sends the front window's app name and title, plus a small grayscale screenshot when needed. The AI answers which of the engineer's projects that work belongs to: **Gemini** in production (API key or Vertex AI), or **Claude** (`TIMEPILOT_AI_PROVIDER`).
2. **Admin page** at `/admin`. Owners and admins of the solidtime organization add a description and keywords to each project (repository names, ticket prefixes such as `BILL-`, customer names, domains). They also see AI usage and cost, and who submitted their day.
3. **Day submissions** (W8): the widget's "Submit day".

It never writes to solidtime, and it never stores screenshots, window titles or tokens.

## Setup

```bash
cd server/autotrack
cp .env.example .env        # git-ignored
# edit .env: GEMINI_API_KEY=...   SOLIDTIME_URL=http://localhost:8000
uv sync
uv run --env-file .env uvicorn --factory autotrack.main:create_app --host 127.0.0.1 --port 8010
```

- Health check: `curl http://localhost:8010/healthz` returns `{"ok":true,"ai":true}` once the key is set.
- Admin page: http://localhost:8010/admin. Sign in with a solidtime personal API token of an owner or admin.
- Without an AI key the service still runs. Widgets then auto-track only from keywords and what they have learned; there are no AI calls.

**Docker** (next to the solidtime stack; start solidtime first so its network exists):

```bash
docker compose up -d --build     # http://127.0.0.1:8010, data in the timepilot-data volume
```

For real use, put HTTPS (Caddy or Nginx) in front. The widget refuses plain HTTP except for localhost.

## How a classification works

1. The widget authenticates with the engineer's own solidtime token (`Authorization: Bearer …`) and organization (`X-TimePilot-Org`). The service asks solidtime who the caller is and which projects they can see; the result is cached for 10 minutes under a hash of the token.
2. The project list (name, client, tasks, admin description and keywords) goes into the prompt in a stable order, so the prompt can be cached. Each project gets a short reference like `P3` or `P3.2`.
3. The first model (**Gemini 3.5 Flash-Lite** or **Claude Haiku 5.5**) answers with structured JSON: reference, category (`work`, `meeting`, `communication`, `break`, `non_work`, `unclear`), confidence and a short neutral activity note.
4. If confidence is below `TIMEPILOT_AI_ESCALATE_BELOW` (0.7), the stronger model re-checks (**Gemini 3.8 Flash** or **Claude Opus 5.5**, with server-side refusal fallback on Opus).
5. The answer is validated. A reference that was not offered (hallucinated, or injected through text on screen) is discarded. Confidence is clamped and the note is cleaned.
6. One usage row is logged per model call: tokens, latency, outcome. There is no content in it.

Everything in `.env.example` can be configured: models, thresholds, effort, capture interval, review time, excluded apps, rate limit.

### Cost

Most of the time the widget decides locally (learned cache and keywords), so a typical engineer needs only a few dozen AI calls a day. With Haiku first and Opus only for unsure answers, the expected cost is about **$1–4 per engineer per month**. The admin page shows the actual estimate from the usage log. Using Opus for every call (`TIMEPILOT_AI_MODEL=claude-opus-5-5`, `TIMEPILOT_AI_ESCALATION_MODEL=`) would cost roughly 10–20 times more.

## Privacy and security

- **Screenshots stay in memory.** Requests over about 900 KB, or without a `Content-Length`, are rejected, so an upload can never spill to a temp file. The image is dropped as soon as the AI has answered.
- **Nothing sensitive is persisted.** No table or log line contains a screenshot, window title or token. A test enforces this.
- **Per-engineer rate limit:** 40 calls per hour by default.
- **Admin endpoints** require the solidtime `owner` or `admin` role. The admin page has a strict Content-Security-Policy and builds the page without `innerHTML`.
- **The AI key (Gemini or Anthropic) exists only here.** Widgets never see it.

## Logs

Every request gets an `X-Request-ID` (returned in the response), and the service writes one line per request: method, path, status, duration and member. It also writes one line per classification: app name, models, outcome, confidence, tokens and latency. Window titles, screenshots and tokens are never logged.

- `TIMEPILOT_LOG_FORMAT=json` gives Google Cloud Logging JSON; `text` is easier to read locally.
- `TIMEPILOT_LOG_LEVEL=DEBUG` adds health checks.

Production log queries are in [docs/DEPLOY_GCP.md](../../docs/DEPLOY_GCP.md#3-logs).

## Troubleshooting

`curl http://localhost:8010/healthz` shows `ai_error` when the AI provider refuses the key. The same message appears on the admin page under AI usage and in the widget's auto-tracking status. Common messages:

- **"API key not valid"** (Gemini): wrong or revoked `GEMINI_API_KEY`. Fix `.env` and restart the service.
- **"RESOURCE_EXHAUSTED"** (Gemini): quota or free-tier limit reached; check billing in Google AI Studio.
- **"Your credit balance is too low…"** (Claude): add credits in the Anthropic Console under Plans & Billing.
- **"invalid x-api-key"** (Claude): wrong or revoked `ANTHROPIC_API_KEY`. Fix `.env` and restart the service.
- **"model: … not found"**: the account has no access to that model. Change `TIMEPILOT_AI_MODEL` or `TIMEPILOT_AI_ESCALATION_MODEL`.

After such an error the service pauses AI calls for 10 minutes instead of failing every widget's request. Restart the service to retry at once. Meanwhile widgets keep tracking with keywords and what they have learned.

## API (for the widget)

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/healthz` | liveness, whether AI is configured |
| GET | `/v1/config` | auto-tracking policy for widgets |
| GET | `/v1/hints` | keywords of projects the caller can see (ETag / 304) |
| POST | `/v1/classify` | multipart: `app`, `title`, optional `image` (JPEG) |
| POST | `/v1/days/{YYYY-MM-DD}/submit` | `{total_secs, entries}` |
| GET | `/v1/organizations` | the token's organizations (admin page sign-in) |
| GET/PUT | `/v1/admin/projects`, `/v1/admin/hints/{project_id}` | project hints (admins) |
| GET | `/v1/admin/usage?days=30`, `/v1/admin/submissions?day=` | cost and submissions (admins) |

## Development

```bash
uv run pytest -q                                   # Claude and solidtime are faked
uv run ruff check . && uv run ruff format --check .
uv run pip-audit
```
