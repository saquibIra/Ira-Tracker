"use strict";
// Admin page. All server data is inserted with textContent / form values, never as HTML.

const $ = (id) => document.getElementById(id);
// The page may be served at /admin/ or behind a prefix such as /ira/admin/.
const BASE = location.pathname.replace(/\/admin\/?.*$/, "");
const store = {
  get: (k) => {
    try { return sessionStorage.getItem(k); } catch { return null; }
  },
  set: (k, v) => {
    try { sessionStorage.setItem(k, v); } catch { /* private mode: keep in memory only */ }
  },
  clear: () => {
    try { sessionStorage.clear(); } catch { /* ignore */ }
  },
};
let token = store.get("token");
let org = store.get("org");

async function api(path, opts = {}) {
  const headers = { Authorization: `Bearer ${token}`, ...(opts.headers || {}) };
  if (org) headers["X-Ira-Org"] = org;
  if (opts.body) headers["Content-Type"] = "application/json";
  const r = await fetch(BASE + path, { ...opts, headers });
  if (!r.ok) {
    let msg = `HTTP ${r.status}`;
    try { msg = (await r.json()).detail || msg; } catch { /* not JSON */ }
    if (r.status === 401) signOut();
    throw new Error(typeof msg === "string" ? msg : JSON.stringify(msg));
  }
  return r.json();
}

function el(tag, props = {}, children = []) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (k === "text") e.textContent = v;
    else if (k === "class") e.className = v;
    else e[k] = v;
  }
  for (const c of children) e.append(c);
  return e;
}

function signOut() {
  token = null;
  org = null;
  store.clear();
  $("app").hidden = true;
  $("signout").hidden = true;
  $("signin").hidden = false;
  $("who").textContent = "";
}

$("signout").addEventListener("click", signOut);

$("signin-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  $("signin-err").textContent = "";
  token = $("token").value.trim();
  try {
    if ($("org-row").hidden) {
      org = null;
      const { organizations } = await api("/v1/organizations");
      const admin = organizations.filter((o) => o.role === "owner" || o.role === "admin");
      if (!admin.length) throw new Error("You are not an owner or admin of any organization.");
      if (admin.length > 1) {
        $("org").replaceChildren(
          ...admin.map((o) => el("option", { value: o.org_id, text: `${o.org_name} (${o.role})` })),
        );
        $("org-row").hidden = false;
        return;
      }
      org = admin[0].org_id;
    } else {
      org = $("org").value;
    }
    store.set("token", token);
    store.set("org", org);
    await start();
  } catch (ex) {
    $("signin-err").textContent = ex.message;
  }
});

document.querySelectorAll(".tab").forEach((b) =>
  b.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((x) => x.classList.toggle("active", x === b));
    for (const t of ["projects", "usage", "days"]) $(`tab-${t}`).hidden = t !== b.dataset.tab;
    if (b.dataset.tab === "usage") loadUsage();
    if (b.dataset.tab === "days") loadDays();
  }),
);

let projects = [];

function renderProjects() {
  const q = $("filter").value.trim().toLowerCase();
  const rows = projects
    .filter((p) => !q || `${p.client || ""} ${p.name}`.toLowerCase().includes(q))
    .map((p) => {
      const desc = el("textarea", { rows: 2, maxLength: 1000, value: p.description || "" });
      const kw = el("input", { value: (p.keywords || []).join(", "), placeholder: "repo-name, ABC-, customer.com" });
      const status = el("span", { class: "muted" });
      const save = el("button", { text: "Save" });
      save.addEventListener("click", async () => {
        save.disabled = true;
        status.textContent = "";
        try {
          const keywords = kw.value.split(",").map((k) => k.trim()).filter(Boolean);
          await api(`/v1/admin/hints/${encodeURIComponent(p.id)}`, {
            method: "PUT",
            body: JSON.stringify({ description: desc.value, keywords }),
          });
          p.description = desc.value;
          p.keywords = keywords;
          status.textContent = "Saved";
        } catch (ex) {
          status.textContent = ex.message;
        } finally {
          save.disabled = false;
        }
      });
      return el("tr", {}, [
        el("td", {}, [el("b", { text: p.name }), el("div", { class: "muted", text: p.client || "Internal" })]),
        el("td", {}, [desc]),
        el("td", {}, [kw]),
        el("td", {}, [save, status]),
      ]);
    });
  $("projects").replaceChildren(...rows);
}

$("filter").addEventListener("input", renderProjects);

async function loadUsage() {
  try {
    const u = await api(`/v1/admin/usage?days=${$("usage-days").value}`);
    $("usage").replaceChildren(
      ...u.models.map((m) =>
        el("tr", {}, [
          el("td", { text: m.model }),
          el("td", { text: String(m.calls) }),
          el("td", { text: String(m.with_screenshot) }),
          el("td", { text: String(m.members) }),
          el("td", { text: m.input_tokens.toLocaleString() }),
          el("td", { text: m.output_tokens.toLocaleString() }),
          el("td", { text: `${m.avg_latency_ms} ms` }),
          el("td", { text: m.estimated_cost_usd == null ? "n/a" : `$${m.estimated_cost_usd.toFixed(2)}` }),
        ]),
      ),
    );
    $("usage-total").textContent = `Estimated total: $${u.estimated_cost_usd.toFixed(2)}`;
    $("usage-error").textContent = u.ai_error ? `AI is paused: ${u.ai_error}` : "";
  } catch (ex) {
    $("err").textContent = ex.message;
  }
}
$("usage-days").addEventListener("change", loadUsage);

const hm = (secs) => `${Math.floor(secs / 3600)}h ${String(Math.floor((secs % 3600) / 60)).padStart(2, "0")}m`;

async function loadDays() {
  try {
    const d = await api(`/v1/admin/submissions?day=${encodeURIComponent($("day").value)}`);
    $("days").replaceChildren(
      ...d.members.map((m) =>
        el("tr", { class: m.submitted ? "" : "missing" }, [
          el("td", { text: m.name || m.member_id }),
          el("td", { text: m.submitted ? new Date(m.submitted.submitted_at).toLocaleString() : "Not submitted" }),
          el("td", { text: m.submitted ? hm(m.submitted.total_secs) : "" }),
          el("td", { text: m.submitted ? String(m.submitted.entries) : "" }),
        ]),
      ),
    );
  } catch (ex) {
    $("err").textContent = ex.message;
  }
}
$("day").addEventListener("change", loadDays);

async function start() {
  $("signin").hidden = true;
  $("app").hidden = false;
  $("signout").hidden = false;
  $("err").textContent = "";
  $("day").value = new Date().toISOString().slice(0, 10);
  try {
    projects = (await api("/v1/admin/projects")).projects;
    $("who").textContent = `${projects.length} projects`;
    renderProjects();
  } catch (ex) {
    $("err").textContent = ex.message;
  }
}

if (token && org) start();
