import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface Item {
  project_id: string;
  task_id: string | null;
  project: string;
  task: string | null;
  client: string | null;
  color: string;
}

interface UiState {
  signed_in: boolean;
  base_url: string | null;
  running: { item: Item | null; description: string; start: string } | null;
  recent: Item[];
  items: Item[];
  pending: number;
  offline: boolean;
  error: string | null;
}

const win = getCurrentWindow();
const app = document.getElementById("app")!;

const esc = (s: string) =>
  s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

const label = (i: Item) => (i.task ? `${i.project} · ${i.task}` : i.project);

function elapsed(start: string): string {
  const secs = Math.max(0, Math.floor((Date.now() - Date.parse(start)) / 1000));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Subsequence fuzzy match; higher is better, -1 means no match. */
function score(query: string, text: string): number {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 0;
  const direct = t.indexOf(q);
  if (direct >= 0) return 1000 - direct;
  let ti = 0;
  let total = 0;
  let streak = 0;
  for (const ch of q) {
    if (ch === " ") continue;
    const found = t.indexOf(ch, ti);
    if (found < 0) return -1;
    streak = found === ti ? streak + 1 : 0;
    total += 10 + streak * 5 - Math.min(found - ti, 10);
    ti = found + 1;
  }
  return total;
}

// ---------- sign-in window ----------

function renderLogin() {
  app.innerHTML = `
    <form id="login" class="login">
      <h1>Sign in to Ira Tracker</h1>
      <p class="muted">Create a personal API token in solidtime:
        <b>Profile → API Tokens → Create</b>, then paste it here.</p>
      <label>Server URL
        <input id="url" value="http://localhost:8000" autocomplete="off" />
      </label>
      <label>API token
        <textarea id="token" rows="4" placeholder="Paste token" autofocus></textarea>
      </label>
      <p id="err" class="error"></p>
      <button id="go" type="submit">Sign in</button>
    </form>`;
  const form = document.getElementById("login") as HTMLFormElement;
  const err = document.getElementById("err")!;
  const go = document.getElementById("go") as HTMLButtonElement;
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    err.textContent = "";
    go.disabled = true;
    go.textContent = "Checking…";
    try {
      await invoke("sign_in", {
        baseUrl: (document.getElementById("url") as HTMLInputElement).value,
        token: (document.getElementById("token") as HTMLTextAreaElement).value,
      });
    } catch (ex) {
      err.textContent = String(ex);
      go.disabled = false;
      go.textContent = "Sign in";
    }
  });
}

// ---------- quick switcher ----------

let state: UiState;
let results: Item[] = [];
let selected = 0;

function renderSwitcher() {
  app.innerHTML = `
    <div class="switcher">
      <div id="running" class="running"></div>
      <input id="q" class="search" placeholder="Switch to project or task…" autocomplete="off" spellcheck="false" />
      <ul id="list" class="list"></ul>
      <input id="desc" class="desc" placeholder="What are you working on? (optional)" autocomplete="off" />
      <div class="hints">
        <span><kbd>↑↓</kbd> select</span><span><kbd>↵</kbd> start</span>
        <span><kbd>Tab</kbd> description</span><span><kbd>Esc</kbd> close</span>
        <span id="sync" class="sync"></span>
      </div>
    </div>`;

  const q = document.getElementById("q") as HTMLInputElement;
  const desc = document.getElementById("desc") as HTMLInputElement;

  q.addEventListener("input", () => {
    selected = 0;
    renderList();
  });
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      selected = Math.min(selected + 1, results.length - 1);
      renderList();
      e.preventDefault();
    } else if (e.key === "ArrowUp") {
      selected = Math.max(selected - 1, 0);
      renderList();
      e.preventDefault();
    } else if (e.key === "Enter") {
      e.preventDefault();
      startSelected();
    }
  };
  q.addEventListener("keydown", onKey);
  desc.addEventListener("keydown", onKey);
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") win.close();
  });
  window.addEventListener("blur", () => win.close());

  refresh().then(() => q.focus());
  listen("state-changed", refresh);
  setInterval(renderRunning, 1000);
}

async function refresh() {
  state = await invoke<UiState>("get_state");
  renderRunning();
  renderList();
}

function renderRunning() {
  const el = document.getElementById("running");
  if (!el || !state) return;
  const sync = document.getElementById("sync")!;
  sync.textContent = state.offline
    ? `offline · ${state.pending} queued`
    : state.pending
      ? `${state.pending} syncing`
      : "";
  if (state.error) sync.textContent = `⚠ ${state.error}`;

  const r = state.running;
  if (!r) {
    el.innerHTML = `<span class="muted">Not tracking</span>`;
    el.dataset.start = "";
    return;
  }
  if (el.dataset.start === r.start) {
    el.querySelector(".clock")!.textContent = elapsed(r.start);
    return;
  }
  el.dataset.start = r.start;
  const name = r.item ? label(r.item) : "No project";
  el.innerHTML = `
    <span class="dot" style="background:${esc(r.item?.color ?? "#999")}"></span>
    <div class="grow">
      <div class="name">${esc(name)}</div>
      <input id="rdesc" class="rdesc" placeholder="Add description…" value="${esc(r.description)}" />
    </div>
    <span class="clock">${elapsed(r.start)}</span>
    <button id="stop" class="stop" title="Stop timer">■</button>`;
  const rdesc = document.getElementById("rdesc") as HTMLInputElement;
  const save = () => {
    if (rdesc.value !== r.description) invoke("set_description", { description: rdesc.value });
  };
  rdesc.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.stopPropagation();
      save();
      (document.getElementById("q") as HTMLInputElement).focus();
    }
  });
  rdesc.addEventListener("blur", save);
  document.getElementById("stop")!.addEventListener("click", async () => {
    await invoke("stop_timer");
    win.close();
  });
}

function renderList() {
  const list = document.getElementById("list");
  if (!list || !state) return;
  const query = (document.getElementById("q") as HTMLInputElement).value.trim();
  if (!query) {
    const recentKeys = new Set(state.recent.map((i) => i.project_id + i.task_id));
    results = [...state.recent, ...state.items.filter((i) => !recentKeys.has(i.project_id + i.task_id))];
  } else {
    results = state.items
      .map((i) => ({ i, s: score(query, `${i.client ?? ""} ${label(i)}`) }))
      .filter((x) => x.s >= 0)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.i);
  }
  results = results.slice(0, 50);
  selected = Math.min(selected, Math.max(results.length - 1, 0));
  const recentCount = query ? 0 : state.recent.length;
  list.innerHTML = results.length
    ? results
        .map(
          (i, idx) => `
        ${idx === 0 && recentCount ? `<li class="section">Recent</li>` : ""}
        ${idx === recentCount && recentCount && !query ? `<li class="section">All projects</li>` : ""}
        <li class="item ${idx === selected ? "sel" : ""}" data-idx="${idx}">
          <span class="dot" style="background:${esc(i.color)}"></span>
          <span class="grow"><b>${esc(i.project)}</b>${i.task ? ` <span class="task">· ${esc(i.task)}</span>` : ""}</span>
          <span class="client">${esc(i.client ?? "Internal")}</span>
        </li>`,
        )
        .join("")
    : `<li class="empty">No matching project or task</li>`;
  list.querySelector(".sel")?.scrollIntoView({ block: "nearest" });
  list.querySelectorAll<HTMLLIElement>("li.item").forEach((li) =>
    li.addEventListener("mousedown", (e) => {
      e.preventDefault();
      selected = Number(li.dataset.idx);
      startSelected();
    }),
  );
}

async function startSelected() {
  const item = results[selected];
  if (!item) return;
  await invoke("start_timer", {
    projectId: item.project_id,
    taskId: item.task_id,
    description: (document.getElementById("desc") as HTMLInputElement).value,
  });
  win.close();
}

if (win.label === "login") renderLogin();
else renderSwitcher();
