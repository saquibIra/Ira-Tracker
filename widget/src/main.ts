import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import {
  type Away,
  type Item,
  type UiState,
  elapsed,
  esc,
  hhmm,
  itemHtml,
  label,
  safeColor,
  search,
} from "./lib";

const win = getCurrentWindow();
const app = document.getElementById("app")!;

// ---------- idle prompt (W6/W7) ----------

async function renderIdle() {
  const away = await invoke<Away | null>("get_away");
  if (!away) {
    win.close();
    return;
  }
  app.innerHTML = `
    <div class="idle">
      <div class="idle-head">
        <b>You were away for ${away.minutes} min</b>
        <span class="muted">${hhmm(away.start)}–${hhmm(away.end)} · the timer kept running on ${esc(away.label)}</span>
      </div>
      <div class="idle-actions">
        <button data-a="keep" title="Count the away time">Keep</button>
        <button data-a="discard" title="Remove the away time">Discard</button>
        <button data-a="assign" title="Move the away time to another project">Assign to…</button>
      </div>
      <div id="assign" class="assign" hidden>
        <input id="aq" class="search" placeholder="Assign away time to…" autocomplete="off" spellcheck="false" />
        <ul id="alist" class="list"></ul>
      </div>
      <p id="err" class="error"></p>
    </div>`;

  const resolve = async (action: string, item?: Item) => {
    try {
      await invoke("resolve_away", {
        action,
        projectId: item?.project_id ?? null,
        taskId: item?.task_id ?? null,
      });
    } catch (ex) {
      document.getElementById("err")!.textContent = String(ex);
    }
  };

  let items: Item[] = [];
  let shown: Item[] = [];
  let sel = 0;
  const aq = document.getElementById("aq") as HTMLInputElement;
  const list = document.getElementById("alist")!;
  const render = () => {
    shown = (aq.value.trim() ? search(aq.value.trim(), items) : items).slice(0, 30);
    sel = Math.min(sel, Math.max(shown.length - 1, 0));
    list.innerHTML = shown.map((i, idx) => itemHtml(i, idx, idx === sel)).join("");
    list.querySelector(".sel")?.scrollIntoView({ block: "nearest" });
    list.querySelectorAll<HTMLLIElement>("li.item").forEach((li) =>
      li.addEventListener("click", () => resolve("assign", shown[Number(li.dataset.idx)])),
    );
  };
  aq.addEventListener("input", () => {
    sel = 0;
    render();
  });
  aq.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") sel = Math.min(sel + 1, shown.length - 1);
    else if (e.key === "ArrowUp") sel = Math.max(sel - 1, 0);
    else if (e.key === "Enter" && shown[sel]) return resolve("assign", shown[sel]);
    else return;
    e.preventDefault();
    render();
  });

  app.querySelectorAll<HTMLButtonElement>(".idle-actions button").forEach((b) =>
    b.addEventListener("click", async () => {
      if (b.dataset.a !== "assign") return resolve(b.dataset.a!);
      const st = await invoke<UiState>("get_state");
      items = st.items;
      document.getElementById("assign")!.hidden = false;
      await win.setSize(new LogicalSize(400, 440));
      render();
      aq.focus();
    }),
  );
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
      <input id="desc" class="desc" placeholder="What are you working on? (optional)" maxlength="500" autocomplete="off" />
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
    <span class="dot" style="background:${safeColor(r.item?.color)}"></span>
    <div class="grow">
      <div class="name">${esc(name)}</div>
      <input id="rdesc" class="rdesc" placeholder="Add description…" maxlength="500" value="${esc(r.description)}" />
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
    results = search(query, state.items);
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
        ${itemHtml(i, idx, idx === selected)}`,
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
else if (win.label === "idle") renderIdle();
else renderSwitcher();
