"""Minimal async client for the solidtime REST API, called with the engineer's own token.

We never store tokens: they arrive with each widget request and are only used to ask
solidtime who the caller is and which projects they can see.
"""

from __future__ import annotations

import httpx2


class SolidtimeError(Exception):
    pass


class Unauthorized(SolidtimeError):
    """The token was rejected by solidtime."""


class Unavailable(SolidtimeError):
    """solidtime could not be reached or answered with a server error."""


MAX_PAGES = 20


class Solidtime:
    def __init__(self, base_url: str, transport: httpx2.AsyncBaseTransport | None = None):
        self._base = base_url.rstrip("/")
        self._api = f"{self._base}/api/v1"
        self._http = httpx2.AsyncClient(timeout=10.0, transport=transport)

    async def aclose(self) -> None:
        await self._http.aclose()

    async def exchange_code(self, client_id: str, code: str, verifier: str, redirect_uri: str) -> dict:
        """OAuth code exchange for the admin page (PKCE, public client). The browser cannot
        call /oauth/token across origins in development, so the service does it."""
        try:
            r = await self._http.post(
                f"{self._base}/oauth/token",
                data={
                    "grant_type": "authorization_code",
                    "client_id": client_id,
                    "code": code,
                    "code_verifier": verifier,
                    "redirect_uri": redirect_uri,
                },
                headers={"Accept": "application/json"},
            )
        except httpx2.HTTPError as e:
            raise Unavailable(type(e).__name__) from e
        if r.status_code >= 500:
            raise Unavailable(f"HTTP {r.status_code}")
        if r.status_code >= 400:
            raise Unauthorized()
        try:
            return r.json()
        except ValueError as e:
            raise Unavailable("invalid JSON") from e

    async def _get(self, token: str, url: str, params: dict | None = None) -> dict:
        try:
            r = await self._http.get(
                url,
                params=params,
                headers={"Authorization": f"Bearer {token}", "Accept": "application/json"},
            )
        except httpx2.HTTPError as e:
            raise Unavailable(type(e).__name__) from e
        if r.status_code == 401:
            raise Unauthorized()
        if r.status_code >= 500:
            raise Unavailable(f"HTTP {r.status_code}")
        if r.status_code == 403:
            raise Unauthorized()
        if r.status_code >= 400:
            raise SolidtimeError(f"HTTP {r.status_code}")
        try:
            return r.json()
        except ValueError as e:
            raise Unavailable("invalid JSON") from e

    async def _list(self, token: str, path: str, params: dict | None = None) -> list[dict]:
        out: list[dict] = []
        url: str | None = f"{self._api}{path}"
        for _ in range(MAX_PAGES):
            if url is None:
                break
            body = await self._get(token, url, params)
            out.extend(body.get("data") or [])
            nxt = (body.get("links") or {}).get("next")
            # Only follow pagination links that stay on the same API.
            url = nxt if isinstance(nxt, str) and nxt.startswith(self._api) else None
            params = None  # the next link already carries the query
        return out

    async def memberships(self, token: str) -> list[dict]:
        return await self._list(token, "/users/me/memberships")

    async def projects(self, token: str, org: str) -> list[dict]:
        return await self._list(token, f"/organizations/{org}/projects")

    async def tasks(self, token: str, org: str) -> list[dict]:
        return await self._list(token, f"/organizations/{org}/tasks", {"done": "false"})

    async def clients(self, token: str, org: str) -> list[dict]:
        return await self._list(token, f"/organizations/{org}/clients")

    async def members(self, token: str, org: str) -> list[dict]:
        return await self._list(token, f"/organizations/{org}/members")
