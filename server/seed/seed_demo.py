"""Seed a solidtime organization with demo customers, projects and tasks.

Usage:
    SOLIDTIME_URL=http://localhost:8000 SOLIDTIME_TOKEN=... python3 seed_demo.py

Safe to re-run: existing clients/projects/tasks with the same name are reused.
"""

import json
import os
import urllib.request

BASE = os.environ.get("SOLIDTIME_URL", "http://localhost:8000").rstrip("/") + "/api/v1"
TOKEN = os.environ["SOLIDTIME_TOKEN"]

# billable_rate is in cents per hour
CLIENTS = {
    "Acme Retail": [
        ("Acme – POS Revamp", "#ef5350", 450000, ["Backend", "Frontend", "QA"]),
        ("Acme – Mobile App", "#ec407a", 450000, ["iOS", "Android", "Design"]),
    ],
    "Northwind Logistics": [
        ("Northwind – Route Optimizer", "#42a5f5", 500000, ["Algorithm", "API", "Support"]),
    ],
    "Globex Health": [
        ("Globex – Patient Portal", "#66bb6a", 400000, ["Development", "Testing", "Meetings"]),
        ("Globex – Data Migration", "#26a69a", 400000, ["Analysis", "Scripts"]),
    ],
}
INTERNAL = [
    ("Internal", "#9e9e9e", ["Admin", "Meetings"]),
    ("Pre-sales", "#ffa726", ["Proposals", "Demos"]),
    ("Learning", "#ab47bc", ["Courses", "R&D"]),
    ("Bench", "#78909c", []),
]


def call(method, path, body=None):
    req = urllib.request.Request(
        BASE + path,
        method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Accept": "application/json",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req) as r:
            return json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        raise SystemExit(f"{method} {path} -> {e.code}: {e.read().decode()[:500]}")


def list_all(org, resource, **params):
    query = "&".join(f"{k}={v}" for k, v in params.items())
    return call("GET", f"/organizations/{org}/{resource}?{query}")["data"]


def main():
    membership = call("GET", "/users/me/memberships")["data"][0]
    org = membership["organization"]["id"]
    member_id = membership["id"]

    call("PUT", f"/organizations/{org}", {
        "name": "Ira Technologies",
        "prevent_overlapping_time_entries": True,
    })

    clients = {c["name"]: c["id"] for c in list_all(org, "clients")}
    projects = {p["name"]: p["id"] for p in list_all(org, "projects", archived="all")}
    tasks = {(t["project_id"], t["name"]) for t in list_all(org, "tasks", done="all")}

    def ensure_project(name, color, client_id, billable, rate, task_names):
        if name not in projects:
            projects[name] = call("POST", f"/organizations/{org}/projects", {
                "name": name,
                "color": color,
                "is_billable": billable,
                "billable_rate": rate,
                "client_id": client_id,
                "is_public": False,
            })["data"]["id"]
            call("POST", f"/organizations/{org}/projects/{projects[name]}/project-members",
                 {"member_id": member_id, "billable_rate": None})
            print("project", name)
        for t in task_names:
            if (projects[name], t) not in tasks:
                call("POST", f"/organizations/{org}/tasks", {"name": t, "project_id": projects[name]})

    for client, client_projects in CLIENTS.items():
        if client not in clients:
            clients[client] = call("POST", f"/organizations/{org}/clients", {"name": client})["data"]["id"]
            print("client", client)
        for name, color, rate, task_names in client_projects:
            ensure_project(name, color, clients[client], True, rate, task_names)

    for name, color, task_names in INTERNAL:
        ensure_project(name, color, None, False, None, task_names)

    print(f"done: org={org} member={member_id}")


if __name__ == "__main__":
    main()
