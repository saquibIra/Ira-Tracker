FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    UV_COMPILE_BYTECODE=1 \
    UV_LINK_MODE=copy

# uv from PyPI (some servers cannot reach ghcr.io)
RUN pip install --no-cache-dir uv==0.12.5

WORKDIR /app
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project
COPY autotrack ./autotrack

RUN useradd --system --uid 10001 autotrack && mkdir /data && chown autotrack /data
USER autotrack
ENV TIMEPILOT_DB_PATH=/data/autotrack.db

EXPOSE 8010
CMD ["/app/.venv/bin/uvicorn", "--factory", "autotrack.main:create_app", "--host", "0.0.0.0", "--port", "8010", "--proxy-headers", "--forwarded-allow-ips", "*", "--no-server-header", "--no-access-log"]
