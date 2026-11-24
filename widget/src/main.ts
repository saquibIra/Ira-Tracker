import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import {
  type AutoView,
  type Away,
  type CaptureView,
  type DayView,
  type EntryView,
  type Item,
  type Membership,
  type SignInOutcome,
  type UiState,
  atLocalTime,
  capturesIn,
  dayRows,
  decidedBy,
  afterTurnOn,
  aiName,
  bakedServer,
  defaultServer,
  duration,
  elapsed,
  esc,
  hhmm,
  itemHtml,
  label,
  MARK_SVG,
  localHhmm,
  mismatch,
  plural,
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

// ---------- auto-tracking settings and consent ----------

async function renderAuto() {
  let v = await invoke<AutoView>("get_auto");
  app.innerHTML = `
    <div class="page">
      <h1>Auto-tracking</h1>
      <p>Time Pilot can log your time to the right project by itself, so you no longer
        switch timers by hand. Please read how it works before you turn it on.</p>
      <ul class="facts">
        <li><b>What it looks at.</b> Every 20 seconds, the name and title of the window in
          front (for example "Code — mmcg-platform"). This stays on your computer.</li>
        <li><b>Screenshots.</b> <span id="shots"></span></li>
        <li><b>AI.</b> When the window title is not enough, that screenshot and the title go
          to Time Pilot's server, which asks <span id="ai"></span> which of your projects it is. The
          server does not keep the screenshot.</li>
        <li><b>What goes to solidtime.</b> Only the project, the times and a short note such
          as "Fixing invoice export".</li>
        <li><b>Never captured.</b> <span id="excluded"></span>; nothing while paused, idle or locked.</li>
        <li><b>You stay in control.</b> Pick a project by hand at any time (auto-tracking
          then follows you for <span id="pin"></span>), pause it for private time from the tray
          menu, and in the end-of-day review fix projects and times, split blocks and delete
          screenshots before you submit.</li>
      </ul>
      <div class="perm" id="perm"></div>
      <p class="status">Status: <b id="status"></b></p>
      <p id="msg" class="notice" role="status"></p>
      <p id="err" class="error"></p>
      <div class="actions">
        <button id="off" class="secondary">Turn off</button>
        <button id="on" class="primary">I agree, turn on auto-tracking</button>
      </div>
    </div>`;
  const err = document.getElementById("err")!;
  const msg = document.getElementById("msg")!;
  const on = document.getElementById("on") as HTMLButtonElement;
  const off = document.getElementById("off") as HTMLButtonElement;

  const paint = () => {
    const every = v.capture_minutes === 1 ? "minute" : plural(v.capture_minutes, "minute");
    document.getElementById("shots")!.textContent =
      v.keep_days > 0
        ? `While tracking, once every ${every}, a small grayscale picture of the screen you are working on. It is kept only on this computer for ${plural(v.keep_days, "day")} so you can check your day in the review (you can delete any of them), then removed automatically.`
        : `Only when the window title is not enough, at most once every ${every}. Screenshots are not kept.`;
    document.getElementById("pin")!.textContent = plural(v.pin_minutes, "minute");
    document.getElementById("ai")!.textContent = aiName(v.ai_provider);
    document.getElementById("excluded")!.textContent = v.excluded.length
      ? `Password managers and personal messengers (${v.excluded.join(", ")})`
      : "Nothing is excluded by your admin";
    document.getElementById("status")!.textContent = v.status;
    // Nothing to edit here once it is on, so the main button just closes the window.
    on.textContent = v.enabled ? "Done" : "I agree, turn on auto-tracking";
    off.hidden = !v.enabled;
    const perm = document.getElementById("perm")!;
    if (v.permission === "granted") {
      perm.innerHTML = `<span>Screen Recording permission: <b>allowed</b></span>`;
    } else if (v.permission === "denied") {
      perm.innerHTML = `
        <span>Screen Recording permission: <b>not allowed yet</b>. Without it only the app
          name is used. Allow <b>Time Pilot</b> in System Settings, then quit and reopen the app.</span>
        <button id="ask" class="secondary">Allow…</button>
        <button id="settings" class="secondary">Open System Settings</button>`;
      document.getElementById("ask")!.addEventListener("click", async () => {
        await invoke("request_screen_permission");
        v = await invoke<AutoView>("get_auto");
        paint();
      });
      document.getElementById("settings")!.addEventListener("click", () => invoke("open_screen_settings"));
    } else {
      perm.innerHTML = `<span class="muted">Screenshots are not supported on this system.</span>`;
    }
  };
  paint();
  // The server's current policy and AI provider, so the text above is what it really does.
  invoke<AutoView>("load_auto")
    .then((fresh) => {
      v = fresh;
      paint();
    })
    .catch(() => {});

  // Confirms what happened, then closes the window (it is destroyed on close).
  const confirmAndClose = (text: string) => {
    msg.textContent = text;
    on.disabled = off.disabled = true;
    setTimeout(() => win.close(), 1200);
  };
  on.addEventListener("click", async () => {
    if (v.enabled) {
      win.close();
      return;
    }
    err.textContent = msg.textContent = "";
    on.disabled = true;
    try {
      v = await invoke<AutoView>("set_auto", { enabled: true });
      paint();
      const r = afterTurnOn(v);
      if (r.close) return confirmAndClose(r.text);
      (r.error ? err : msg).textContent = r.text;
    } catch (ex) {
      err.textContent = String(ex);
    }
    on.disabled = false;
  });
  off.addEventListener("click", async () => {
    err.textContent = msg.textContent = "";
    try {
      v = await invoke<AutoView>("set_auto", { enabled: false });
      paint();
      confirmAndClose("Auto-tracking is off.");
    } catch (ex) {
      err.textContent = String(ex);
    }
  });
  listen("state-changed", async () => {
    v = await invoke<AutoView>("get_auto");
    paint();
  });
}

// ---------- end-of-day review (W8) ----------

async function renderReview() {
  let offset = 0;
  let day: DayView;
  let busy = false; // an editor, the picker or the viewer is open: don't repaint under the user
  let picking: string | null = null;
  let shown: Item[] = [];
  let sel = 0;
  let viewing = -1;
  let tab: "log" | "activity" = location.hash === "#activity" ? "activity" : "log";

  app.innerHTML = `
    <div class="review">
      <header>
        <button id="prev" class="nav" title="Previous day">‹</button>
        <div class="grow">
          <h1 id="day"></h1>
          <span id="sum" class="muted"></span>
        </div>
        <button id="next" class="nav" title="Next day">›</button>
      </header>
      <nav class="tabs">
        <button data-tab="log" class="tab">Time log</button>
        <button data-tab="activity" class="tab">Activity &amp; screenshots</button>
      </nav>
      <div id="banner" class="banner" hidden>
        <span id="btext" class="grow"></span>
        <button id="bsettings" class="secondary">Open System Settings</button>
        <button id="brestart" class="primary">Restart Time Pilot</button>
      </div>
      <ul id="rows" class="rows"></ul>
      <ul id="acts" class="acts" hidden></ul>
      <div id="picker" class="picker" hidden>
        <div id="ptitle" class="ptitle muted"></div>
        <input id="pq" class="search" placeholder="Choose project or task…" autocomplete="off" spellcheck="false" />
        <ul id="plist" class="list"></ul>
      </div>
      <div id="viewer" class="viewer" hidden>
        <div class="vbar">
          <b id="vtime"></b>
          <span id="vwin" class="muted grow"></span>
          <button id="vclose" class="nav" title="Close (Esc)">✕</button>
        </div>
        <div class="vimg">
          <button id="vprev" class="nav" title="Previous (←)">‹</button>
          <img id="vimg" alt="Screenshot" />
          <p id="vnone" class="vnone muted" hidden>No screenshot for this check (Screen Recording was not allowed,
            or the capture failed). The window and the classification are still shown below.</p>
          <button id="vnext" class="nav" title="Next (→)">›</button>
        </div>
        <div class="vinfo">
          <span id="vdot" class="dot"></span>
          <b id="vproj"></b>
          <span id="vby" class="muted"></span>
          <span id="vnote" class="muted"></span>
        </div>
        <div class="actions">
          <button id="vdelete" class="secondary">Delete screenshot</button>
          <button id="vsplit" class="primary">From here on: choose project…</button>
        </div>
      </div>
      <footer>
        <span id="note" class="muted grow"></span>
        <button id="submit" class="primary">Submit day</button>
      </footer>
      <p id="err" class="error"></p>
    </div>`;
  const err = document.getElementById("err")!;
  const rowsEl = document.getElementById("rows")!;
  const picker = document.getElementById("picker")!;
  const pq = document.getElementById("pq") as HTMLInputElement;
  const plist = document.getElementById("plist")!;
  const submit = document.getElementById("submit") as HTMLButtonElement;
  const viewer = document.getElementById("viewer")!;
  const vimg = document.getElementById("vimg") as HTMLImageElement;

  const call = async (cmd: string, args: Record<string, unknown>): Promise<boolean> => {
    err.textContent = "";
    try {
      await invoke(cmd, args);
      return true;
    } catch (ex) {
      err.textContent = String(ex);
      return false;
    }
  };

  const load = async () => {
    day = await invoke<DayView>("get_day", { offset });
    paint();
  };

  const entryOf = (c: CaptureView): EntryView | undefined =>
    day.entries.find((e) => capturesIn(e, [c]).length > 0);

  const acts = document.getElementById("acts")!;

  const paintActivity = () => {
    // Newest first: one row per check, with the screenshot, the window, the
    // classification and the time block it was logged in.
    const order = day.captures.map((c, i) => ({ c, i })).reverse();
    acts.innerHTML = order.length
      ? order
          .map(({ c, i }) => {
            const e = entryOf(c);
            const other = e ? mismatch(c, e) : false;
            const logged = e
              ? `<span class="muted">Logged as</span> <b>${e.item ? esc(label(e.item)) : "Needs review"}</b>
                 <span class="muted">${hhmm(e.start)}–${e.end ? hhmm(e.end) : "now"}</span>`
              : `<span class="muted">Not in a logged block</span>`;
            return `
              <li class="act ${other ? "other" : ""}" data-idx="${i}" title="Open">
                <span class="atime">${hhmm(c.taken_at)}</span>
                <span class="athumb">${c.thumb ? `<img src="${esc(c.thumb)}" alt="" />` : `<span class="nothumb">no picture</span>`}</span>
                <div class="grow">
                  <div class="awin">${esc(c.app)}${c.title ? ` — ${esc(c.title)}` : ""}</div>
                  <div class="adec">
                    <span class="dot" style="background:${c.item ? safeColor(c.item.color) : "transparent"}"></span>
                    <b>${esc(kindLabel(c))}</b>
                    <span class="muted">· ${esc(decidedBy(c))}${c.note ? ` · “${esc(c.note)}”` : ""}</span>
                  </div>
                </div>
                <div class="alog">${logged}${other ? `<div class="warn">Screenshot shows another project</div>` : ""}</div>
              </li>`;
          })
          .join("")
      : `<li class="empty">No activity recorded on this day yet. While auto-tracking is on, a record with a
           screenshot is added every few minutes.</li>`;
    acts.querySelectorAll<HTMLLIElement>("li.act").forEach((li) =>
      li.addEventListener("click", () => openViewer(Number(li.dataset.idx))),
    );
  };

  const paint = () => {
    document.getElementById("day")!.textContent = offset === 0 ? `Today, ${day.label}` : day.label;
    const review = day.review_count ? ` · ${day.review_count} need${day.review_count === 1 ? "s" : ""} review` : "";
    const pictures = day.captures.filter((c) => c.thumb).length;
    const shots = day.captures.length ? ` · ${plural(pictures, "screenshot")}` : "";
    document.getElementById("sum")!.textContent = `${duration(day.total_secs)} tracked${review}${shots}`;
    (document.getElementById("next") as HTMLButtonElement).disabled = offset === 0;
    (document.getElementById("prev") as HTMLButtonElement).disabled = offset >= 6;
    const denied = day.screen_permission === "denied" && day.keep_days > 0;
    const banner = document.getElementById("banner")!;
    banner.hidden = !denied && !day.capture_error;
    document.getElementById("btext")!.textContent = denied
      ? "No screenshots: macOS has not given Time Pilot Screen Recording access. In System Settings → Privacy & " +
        "Security → Screen & System Audio Recording, turn Time Pilot on (if it is already on, remove it with – and " +
        "add it again with +). Then restart Time Pilot."
      : `Screenshots are failing: ${day.capture_error ?? ""}`;
    document.querySelectorAll<HTMLButtonElement>(".tabs .tab").forEach((b) =>
      b.classList.toggle("active", b.dataset.tab === tab),
    );
    rowsEl.hidden = tab !== "log";
    acts.hidden = tab !== "activity";
    if (tab === "activity") paintActivity();

    const rows = dayRows(day.entries);
    rowsEl.innerHTML = rows.length
      ? rows
          .map((r) => {
            if (r.kind === "gap") {
              return `<li class="gap">${hhmm(r.start)}–${hhmm(r.end)} · ${duration(r.secs)} not tracked</li>`;
            }
            const e = r.entry;
            const name = e.item ? esc(label(e.item)) : "Needs review: choose a project";
            const caps = capturesIn(e, day.captures);
            const strip = caps.length
              ? `<div class="shots">${caps
                  .map(
                    (c) => `
                <button class="shot ${mismatch(c, e) ? "other" : ""}" data-cap="${esc(c.id)}"
                  title="${esc(`${hhmm(c.taken_at)} · ${c.app} — ${c.title}`)}">
                  ${c.thumb ? `<img src="${esc(c.thumb)}" alt="" />` : `<span class="nothumb"></span>`}
                  <span class="stime">${hhmm(c.taken_at)}</span>
                </button>`,
                  )
                  .join("")}</div>`
              : "";
            return `
              <li class="entry ${e.needs_review ? "needs" : ""}" data-id="${esc(e.local_id)}">
                <div class="line">
                  <button class="time" title="Change start and end">${hhmm(e.start)}–${e.end ? hhmm(e.end) : "now"}</button>
                  <span class="dot" style="background:${e.item ? safeColor(e.item.color) : "transparent"}"></span>
                  <div class="grow">
                    <button class="proj" title="Change project">${name}</button>
                    <input class="edesc" maxlength="500" placeholder="Add a description…" value="${esc(e.description)}" />
                    ${e.error ? `<div class="error">Sync: ${esc(e.error)}</div>` : ""}
                  </div>
                  <span class="dur">${duration(r.secs)}</span>
                  <span class="tag">${e.auto ? "auto" : "manual"}</span>
                  <button class="icon split" title="Split this block in two">✂</button>
                  <button class="icon del" title="Delete this block">×</button>
                </div>
                <div class="editor" hidden></div>
                ${strip}
              </li>`;
          })
          .join("")
      : `<li class="empty">Nothing tracked on this day.</li>`;

    rowsEl.querySelectorAll<HTMLLIElement>("li.entry").forEach((li) => {
      const id = li.dataset.id!;
      const e = day.entries.find((x) => x.local_id === id)!;
      li.querySelector(".proj")!.addEventListener("click", () => openPicker(id));
      li.querySelector(".time")!.addEventListener("click", () => openTimes(li, e));
      li.querySelector(".split")!.addEventListener("click", () => openSplit(li, e));
      const d = li.querySelector(".edesc") as HTMLInputElement;
      const save = () => {
        if (d.value !== e.description) call("update_entry", { localId: id, projectId: null, taskId: null, description: d.value });
      };
      d.addEventListener("blur", save);
      d.addEventListener("keydown", (ev) => {
        if (ev.key === "Enter") d.blur();
      });
      const del = li.querySelector(".del") as HTMLButtonElement;
      del.addEventListener("click", () => {
        if (del.dataset.armed) return void call("delete_entry", { localId: id });
        del.dataset.armed = "1";
        del.textContent = "Delete?";
        setTimeout(() => {
          delete del.dataset.armed;
          del.textContent = "×";
        }, 3000);
      });
      li.querySelectorAll<HTMLButtonElement>(".shot").forEach((b) =>
        b.addEventListener("click", () => openViewer(day.captures.findIndex((c) => c.id === b.dataset.cap))),
      );
    });

    const note = document.getElementById("note")!;
    if (day.submitted_at) note.textContent = `Submitted at ${hhmm(day.submitted_at)}. You can still edit and submit again.`;
    else if (day.review_count) note.textContent = "Choose a project for each highlighted block, or delete it, then submit.";
    else if (day.pending) note.textContent = `${day.pending} change(s) waiting to sync.`;
    else note.textContent = "Check the times and screenshots, then submit your day.";
    submit.disabled = day.review_count > 0;
    submit.textContent = day.submitted_at ? "Submit again" : "Submit day";
  };

  // ----- inline editors: times and split -----

  const closeEditor = (ed: HTMLElement) => {
    ed.hidden = true;
    ed.innerHTML = "";
    busy = false;
  };

  const openTimes = (li: HTMLLIElement, e: EntryView) => {
    const ed = li.querySelector<HTMLDivElement>(".editor")!;
    busy = true;
    ed.hidden = false;
    ed.innerHTML = `
      <label>Start <input type="time" class="tstart" value="${localHhmm(e.start)}" /></label>
      <label>End ${e.end ? `<input type="time" class="tend" value="${localHhmm(e.end)}" />` : `<span class="muted">still running</span>`}</label>
      <span class="muted hint">The block next to it moves with the change.</span>
      <button class="secondary cancel">Cancel</button>
      <button class="primary save">Save times</button>`;
    ed.querySelector(".cancel")!.addEventListener("click", () => closeEditor(ed));
    ed.querySelector(".save")!.addEventListener("click", async () => {
      const start = atLocalTime(e.start, (ed.querySelector(".tstart") as HTMLInputElement).value);
      const endInput = ed.querySelector(".tend") as HTMLInputElement | null;
      const end = e.end && endInput ? atLocalTime(e.end, endInput.value) : null;
      if (!start || (e.end && !end)) {
        err.textContent = "Enter times as HH:MM";
        return;
      }
      if (await call("set_entry_times", { localId: e.local_id, start, end })) {
        closeEditor(ed);
        await load();
      }
    });
  };

  const splitAt = async (e: EntryView, at: string, from: string) => {
    try {
      err.textContent = "";
      const second = await invoke<string>("split_entry", { localId: e.local_id, at });
      await load();
      openPicker(second, `Project from ${from} on`);
    } catch (ex) {
      err.textContent = String(ex);
    }
  };

  const openSplit = (li: HTMLLIElement, e: EntryView) => {
    const ed = li.querySelector<HTMLDivElement>(".editor")!;
    const mid = new Date((Date.parse(e.start) + (e.end ? Date.parse(e.end) : Date.now())) / 2).toISOString();
    busy = true;
    ed.hidden = false;
    ed.innerHTML = `
      <label>Split at <input type="time" class="tat" value="${localHhmm(mid)}" /></label>
      <span class="muted hint">Then choose the project for the part after this time.</span>
      <button class="secondary cancel">Cancel</button>
      <button class="primary save">Split</button>`;
    ed.querySelector(".cancel")!.addEventListener("click", () => closeEditor(ed));
    ed.querySelector(".save")!.addEventListener("click", async () => {
      const value = (ed.querySelector(".tat") as HTMLInputElement).value;
      const at = atLocalTime(e.start, value);
      if (!at) {
        err.textContent = "Enter the time as HH:MM";
        return;
      }
      closeEditor(ed);
      await splitAt(e, at, value);
    });
  };

  // ----- screenshot viewer -----

  const kindLabel = (c: CaptureView): string => {
    if (c.kind === "project") return c.item ? label(c.item) : "A project you can no longer see";
    if (c.kind === "review") return "Needs review";
    if (c.kind === "neutral") return "Chat, email or break (kept the current project)";
    return "Not recognised yet";
  };

  const showCapture = async () => {
    const c = day.captures[viewing];
    if (!c) return closeViewer();
    document.getElementById("vtime")!.textContent = hhmm(c.taken_at);
    document.getElementById("vwin")!.textContent = `${c.app}${c.title ? ` — ${c.title}` : ""}`;
    document.getElementById("vproj")!.textContent = kindLabel(c);
    document.getElementById("vdot")!.style.background = c.item ? safeColor(c.item.color) : "transparent";
    document.getElementById("vby")!.textContent = `· decided by ${decidedBy(c)}`;
    document.getElementById("vnote")!.textContent = c.note ? `· “${c.note}”` : "";
    (document.getElementById("vprev") as HTMLButtonElement).disabled = viewing <= 0;
    (document.getElementById("vnext") as HTMLButtonElement).disabled = viewing >= day.captures.length - 1;
    (document.getElementById("vsplit") as HTMLButtonElement).disabled = !entryOf(c);
    vimg.removeAttribute("src");
    vimg.hidden = !c.thumb;
    document.getElementById("vnone")!.hidden = !!c.thumb;
    (document.getElementById("vdelete") as HTMLButtonElement).textContent = c.thumb ? "Delete screenshot" : "Delete record";
    if (!c.thumb) return;
    try {
      const url = await invoke<string>("get_capture", { id: c.id });
      if (url.startsWith("data:image/jpeg;base64,") && day.captures[viewing]?.id === c.id) vimg.src = url;
    } catch (ex) {
      err.textContent = String(ex);
    }
  };
  const openViewer = (i: number) => {
    if (i < 0) return;
    viewing = i;
    busy = true;
    viewer.hidden = false;
    showCapture();
  };
  const closeViewer = () => {
    viewer.hidden = true;
    vimg.removeAttribute("src");
    viewing = -1;
    busy = false;
  };
  document.getElementById("vclose")!.addEventListener("click", closeViewer);
  document.getElementById("vprev")!.addEventListener("click", () => {
    if (viewing > 0) openViewer(viewing - 1);
  });
  document.getElementById("vnext")!.addEventListener("click", () => {
    if (viewing < day.captures.length - 1) openViewer(viewing + 1);
  });
  document.getElementById("vdelete")!.addEventListener("click", async () => {
    const c = day.captures[viewing];
    if (!c || !(await call("delete_capture", { id: c.id }))) return;
    const keep = viewing;
    day = await invoke<DayView>("get_day", { offset });
    paint();
    if (day.captures.length) openViewer(Math.min(keep, day.captures.length - 1));
    else closeViewer();
  });
  document.getElementById("vsplit")!.addEventListener("click", async () => {
    const c = day.captures[viewing];
    const e = c && entryOf(c);
    if (!c || !e) return;
    closeViewer();
    if (Date.parse(c.taken_at) <= Date.parse(e.start)) openPicker(e.local_id, "Project for this block");
    else await splitAt(e, c.taken_at, hhmm(c.taken_at));
  });
  document.addEventListener("keydown", (ev) => {
    if (viewer.hidden) return;
    if (ev.key === "Escape") closeViewer();
    else if (ev.key === "ArrowLeft" && viewing > 0) openViewer(viewing - 1);
    else if (ev.key === "ArrowRight" && viewing < day.captures.length - 1) openViewer(viewing + 1);
  });

  // ----- project picker -----

  const renderPick = () => {
    const q = pq.value.trim();
    shown = (q ? search(q, day.items) : day.items).slice(0, 40);
    sel = Math.min(sel, Math.max(shown.length - 1, 0));
    plist.innerHTML = shown.map((i, idx) => itemHtml(i, idx, idx === sel)).join("");
    plist.querySelector(".sel")?.scrollIntoView({ block: "nearest" });
    plist.querySelectorAll<HTMLLIElement>("li.item").forEach((li) =>
      li.addEventListener("mousedown", (ev) => {
        ev.preventDefault();
        choose(shown[Number(li.dataset.idx)]);
      }),
    );
  };
  const openPicker = (id: string, title = "Project for this block") => {
    picking = id;
    busy = true;
    picker.hidden = false;
    document.getElementById("ptitle")!.textContent = title;
    pq.value = "";
    sel = 0;
    renderPick();
    pq.focus();
  };
  const closePicker = () => {
    picking = null;
    picker.hidden = true;
    busy = false;
  };
  const choose = async (i: Item | undefined) => {
    if (!i || !picking) return;
    const id = picking;
    closePicker();
    await call("update_entry", { localId: id, projectId: i.project_id, taskId: i.task_id, description: null });
    await load();
  };
  pq.addEventListener("input", () => {
    sel = 0;
    renderPick();
  });
  pq.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") sel = Math.min(sel + 1, shown.length - 1);
    else if (e.key === "ArrowUp") sel = Math.max(sel - 1, 0);
    else if (e.key === "Enter") return void choose(shown[sel]);
    else if (e.key === "Escape") return closePicker();
    else return;
    e.preventDefault();
    renderPick();
  });
  pq.addEventListener("blur", closePicker);

  document.getElementById("prev")!.addEventListener("click", () => {
    offset = Math.min(offset + 1, 6);
    load();
  });
  document.getElementById("next")!.addEventListener("click", () => {
    offset = Math.max(offset - 1, 0);
    load();
  });
  submit.addEventListener("click", async () => {
    submit.disabled = true;
    await call("submit_day", { offset });
    await load();
  });
  document.querySelectorAll<HTMLButtonElement>(".tabs .tab").forEach((b) =>
    b.addEventListener("click", () => {
      tab = b.dataset.tab === "activity" ? "activity" : "log";
      paint();
    }),
  );
  document.getElementById("bsettings")!.addEventListener("click", () => invoke("open_screen_settings"));
  document.getElementById("brestart")!.addEventListener("click", () => invoke("restart_app"));
  listen<string>("review-tab", (ev) => {
    tab = ev.payload === "activity" ? "activity" : "log";
    if (day) paint();
  });
  listen("state-changed", () => {
    if (!busy) load();
  });
  await load();
}

// ---------- review reminder (W8, 18:30) ----------

async function renderNudge() {
  const day = await invoke<DayView>("get_day", { offset: 0 });
  const review = day.review_count ? ` · ${day.review_count} need${day.review_count === 1 ? "s" : ""} review` : "";
  app.innerHTML = `
    <div class="nudge">
      <div class="grow">
        <b>Time to review your day</b>
        <span class="muted">${duration(day.total_secs)} tracked${review}</span>
      </div>
      <div class="actions">
        <button id="later" class="secondary">Later</button>
        <button id="open" class="primary">Review</button>
      </div>
    </div>`;
  document.getElementById("later")!.addEventListener("click", () => win.close());
  document.getElementById("open")!.addEventListener("click", async () => {
    await invoke("open_review");
    win.close();
  });
  // Non-modal toast: disappears after 8 s unless the pointer is on it.
  let hovered = false;
  app.addEventListener("mouseenter", () => (hovered = true));
  app.addEventListener("mouseleave", () => (hovered = false));
  const close = () => (hovered ? setTimeout(close, 2000) : win.close());
  setTimeout(close, 8000);
}

// ---------- sign-in window ----------

// Welcome window: one "Sign in" button that finishes in the browser (see oauth.rs).
function renderLogin() {
  const baked = bakedServer(import.meta.env.VITE_TIMEPILOT_SERVER_URL);
  app.innerHTML = `
    <div class="welcome">
      <div class="brand">
        ${MARK_SVG}
        <h1>Time Pilot</h1>
        <p class="tagline">Your time, tracked to the right project. Automatically.</p>
      </div>
      <ul class="points" id="points">
        <li>Recognises the project from the work on your screen. No timers to start.</li>
        <li>Screenshots stay on this computer. Only projects and times are shared.</li>
        <li>Check and submit your day in a minute.</li>
      </ul>
      <div class="orgs" id="orgs" hidden></div>
      <div class="actions">
        <label class="server" id="server-row">Server
          <input id="url" autocomplete="off" spellcheck="false" />
        </label>
        <p id="err" class="error" role="alert"></p>
        <button id="go" class="primary-btn" type="button">Sign in</button>
        <button id="cancel" class="link-btn" type="button" hidden>Cancel</button>
        <p class="fine" id="fine">Opens your browser to sign in with your Time Pilot account.</p>
      </div>
    </div>`;
  const err = document.getElementById("err")!;
  const go = document.getElementById("go") as HTMLButtonElement;
  const cancel = document.getElementById("cancel") as HTMLButtonElement;
  const fine = document.getElementById("fine")!;
  const url = document.getElementById("url") as HTMLInputElement;
  const serverRow = document.getElementById("server-row")!;
  const points = document.getElementById("points")!;
  const orgs = document.getElementById("orgs")!;
  url.value = baked || defaultServer(undefined);
  serverRow.hidden = Boolean(baked); // only development builds choose a server

  const idle = () => {
    go.disabled = false;
    go.textContent = "Sign in";
    cancel.hidden = true;
    fine.textContent = "Opens your browser to sign in with your Time Pilot account.";
  };

  const chooseOrganization = (list: Membership[]) => {
    points.hidden = true;
    go.hidden = true;
    cancel.hidden = true;
    fine.textContent = "You belong to several organizations. Time is tracked in the one you choose.";
    orgs.innerHTML =
      "<h2>Choose an organization</h2>" +
      list
        .map(
          (o) =>
            `<button class="org" type="button" data-id="${esc(o.org_id)}">` +
            `<span>${esc(o.org_name)}</span><span class="role">${esc(o.role)}</span></button>`,
        )
        .join("");
    orgs.hidden = false;
    orgs.querySelectorAll<HTMLButtonElement>(".org").forEach((b) =>
      b.addEventListener("click", async () => {
        err.textContent = "";
        orgs.querySelectorAll<HTMLButtonElement>(".org").forEach((x) => (x.disabled = true));
        try {
          await invoke("choose_organization", { organizationId: b.dataset.id });
        } catch (ex) {
          err.textContent = String(ex);
          orgs.querySelectorAll<HTMLButtonElement>(".org").forEach((x) => (x.disabled = false));
        }
      }),
    );
  };

  go.addEventListener("click", async () => {
    err.textContent = "";
    go.disabled = true;
    go.innerHTML = '<span class="spinner"></span>Waiting for your browser…';
    cancel.hidden = false;
    fine.textContent = "Finish signing in in your browser, then come back here.";
    try {
      const result = await invoke<SignInOutcome>("sign_in", { baseUrl: url.value });
      if (result.status === "choose") chooseOrganization(result.organizations);
      // "done": the app closes this window and continues.
    } catch (ex) {
      err.textContent = String(ex);
      idle();
    }
  });
  cancel.addEventListener("click", () => invoke("cancel_sign_in"));
  go.focus();
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
  const name = r.item ? label(r.item) : "Needs review";
  el.innerHTML = `
    <span class="dot" style="background:${safeColor(r.item?.color)}"></span>
    <div class="grow">
      <div class="name">${esc(name)}${r.auto ? ` <span class="tag">auto</span>` : ""}</div>
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
else if (win.label === "auto") renderAuto();
else if (win.label === "review") renderReview();
else if (win.label === "nudge") renderNudge();
else renderSwitcher();
