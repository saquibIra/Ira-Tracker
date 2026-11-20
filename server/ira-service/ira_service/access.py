"""Who is calling, which projects they can see, and how often they may call.

Identity comes from solidtime: the widget sends the engineer's own solidtime token and
we ask solidtime for the caller's membership. Results are cached briefly, keyed by a
hash of the token (the token itself is never stored or logged).
"""

from __future__ import annotations

import hashlib
import time
from collections import deque
from dataclasses import dataclass, field

from .classify import Project
from .solidtime import Solidtime

MAX_CACHE_ENTRIES = 5000


class NotMember(Exception):
    """The token is valid but not for the requested organization."""


@dataclass(frozen=True)
class Caller:
    token: str = field(repr=False)
    org_id: str
    member_id: str
    role: str

    @property
    def key(self) -> str:
        return hashlib.sha256(f"{self.org_id}\n{self.token}".encode()).hexdigest()


class TtlCache:
    def __init__(self, ttl: float, clock=time.monotonic):
        self._ttl = ttl
        self._clock = clock
        self._data: dict[str, tuple[float, object]] = {}

    def get(self, key: str):
        hit = self._data.get(key)
        if hit is None or hit[0] < self._clock():
            self._data.pop(key, None)
            return None
        return hit[1]

    def put(self, key: str, value) -> None:
        now = self._clock()
        if len(self._data) >= MAX_CACHE_ENTRIES:
            for k in [k for k, (exp, _) in self._data.items() if exp < now]:
                del self._data[k]
            if len(self._data) >= MAX_CACHE_ENTRIES:
                self._data.pop(next(iter(self._data)))
        self._data[key] = (now + self._ttl, value)

    def drop(self, key: str) -> None:
        self._data.pop(key, None)


class Auth:
    def __init__(self, solidtime: Solidtime, ttl: float):
        self._st = solidtime
        self._cache = TtlCache(ttl)

    async def caller(self, token: str, org_id: str) -> Caller:
        key = hashlib.sha256(f"{org_id}\n{token}".encode()).hexdigest()
        hit = self._cache.get(key)
        if hit is not None:
            return hit
        for m in await self._st.memberships(token):
            org = m.get("organization") or {}
            if org.get("id") == org_id:
                caller = Caller(token=token, org_id=org_id, member_id=m["id"], role=m.get("role", ""))
                self._cache.put(key, caller)
                return caller
        raise NotMember()


class Catalogs:
    """The caller's visible, active projects with their tasks and client names."""

    def __init__(self, solidtime: Solidtime, ttl: float):
        self._st = solidtime
        self._cache = TtlCache(ttl)

    async def projects(self, c: Caller) -> list[Project]:
        hit = self._cache.get(c.key)
        if hit is not None:
            return hit
        projects = await self._st.projects(c.token, c.org_id)
        tasks = await self._st.tasks(c.token, c.org_id)
        clients = {x["id"]: x.get("name", "") for x in await self._st.clients(c.token, c.org_id)}
        by_project: dict[str, list[tuple[str, str]]] = {}
        for t in tasks:
            if not t.get("is_done"):
                by_project.setdefault(t.get("project_id"), []).append((t["id"], t.get("name", "")))
        out = [
            Project(
                id=p["id"],
                name=p.get("name", ""),
                client=clients.get(p.get("client_id")) if p.get("client_id") else None,
                tasks=tuple(by_project.get(p["id"], ())),
            )
            for p in projects
            if not p.get("is_archived")
        ]
        self._cache.put(c.key, out)
        return out

    def forget(self, c: Caller) -> None:
        self._cache.drop(c.key)


class RateLimiter:
    """Sliding one-hour window per caller."""

    def __init__(self, per_hour: int, clock=time.monotonic):
        self._limit = per_hour
        self._clock = clock
        self._hits: dict[str, deque[float]] = {}

    def allow(self, key: str) -> tuple[bool, int]:
        """Returns (allowed, seconds until the next call is allowed)."""
        now = self._clock()
        q = self._hits.setdefault(key, deque())
        while q and q[0] <= now - 3600:
            q.popleft()
        if len(q) >= self._limit:
            return False, max(1, int(q[0] + 3600 - now) + 1)
        q.append(now)
        if len(self._hits) > MAX_CACHE_ENTRIES:
            for k in [k for k, v in self._hits.items() if not v or v[-1] <= now - 3600]:
                del self._hits[k]
        return True, 0
