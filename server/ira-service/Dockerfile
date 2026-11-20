FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy

COPY --from=ghcr.io/astral-sh/uv:0.12.5 /uv /usr/local/bin/uv

WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project
COPY ira_service ./ira_service

RUN useradd --system --uid 10001 ira && mkdir /data && chown ira /data
USER ira
ENV IRA_DB_PATH=/data/ira-service.db

EXPOSE 8010
CMD ["/app/.venv/bin/uvicorn", "--factory", "ira_service.main:create_app", "--host", "0.0.0.0", "--port", "8010", "--proxy-headers", "--forwarded-allow-ips", "*", "--no-server-header", "--no-access-log"]
