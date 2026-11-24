// Pure helpers shared by the windows. No Tauri or DOM access here, so they are unit-tested.

export interface Item {
  project_id: string;
  task_id: string | null;
  project: string;
  task: string | null;
  client: string | null;
  color: string;
}

export interface UiState {
  signed_in: boolean;
  base_url: string | null;
  running: { item: Item | null; description: string; start: string; auto: boolean } | null;
  recent: Item[];
  items: Item[];
  pending: number;
  offline: boolean;
  error: string | null;
  auto: string;
}

export interface AutoView {
  enabled: boolean;
  service_url: string | null;
  permission: "granted" | "denied" | "unsupported";
  status: string;
  paused: boolean;
  server_ready: boolean;
  capture_minutes: number;
  excluded: string[];
  keep_days: number;
  pin_minutes: number;
  /** The AI the server asks ("gemini", "anthropic"); empty until the server was reached. */
  ai_provider: string;
}

/** A screenshot kept on this computer for the review, with what auto-tracking decided. */
export interface CaptureView {
  id: string;
  taken_at: string;
  app: string;
  title: string;
  kind: string; // project | review | neutral | pending
  item: Item | null;
  note: string;
  confidence: number | null;
  source: string; // ai | keywords | cache | pending
  thumb: string | null;
}

export interface EntryView {
  local_id: string;
  start: string;
  end: string | null;
  item: Item | null;
  description: string;
  auto: boolean;
  needs_review: boolean;
  running: boolean;
  error: string | null;
}

export interface DayView {
  date: string;
  label: string;
  offset: number;
  entries: EntryView[];
  total_secs: number;
  review_count: number;
  submitted_at: string | null;
  pending: number;
  items: Item[];
  captures: CaptureView[];
  screen_permission: "granted" | "denied" | "unsupported";
  keep_days: number;
  capture_error: string | null;
}

export interface Membership {
  org_id: string;
  org_name: string;
  member_id: string;
  role: string;
}

/** What `sign_in` returns once the browser comes back. */
export type SignInOutcome = { status: "done" } | { status: "choose"; organizations: Membership[] };

/** The Time Pilot mark (same drawing as scripts/brand/make_brand.py). */
export const MARK_SVG =
  '<svg viewBox="0 0 48 48" aria-hidden="true"><rect width="48" height="48" rx="12" fill="#0D9488"/>' +
  '<circle cx="24" cy="24" r="14.5" fill="none" stroke="#fff" stroke-opacity=".35" stroke-width="3"/>' +
  '<path d="M24 24V14M24 24l9-5.2" stroke="#fff" stroke-width="3.2" stroke-linecap="round"/>' +
  '<path d="M36.1 17l-1.9 4.8-3.2-5.5z" fill="#fff"/><circle cx="24" cy="24" r="3" fill="#fff"/></svg>';

/** The server a build was made for (VITE_TIMEPILOT_SERVER_URL); empty for development builds. */
export const bakedServer = (configured: string | undefined): string =>
  (configured ?? "").trim().replace(/\/+$/, "");

export interface Away {
  start: string;
  end: string;
  minutes: number;
  entry: string;
  label: string;
}

/** Escapes text for use inside HTML element content and quoted attributes. */
export const esc = (s: string): string =>
  s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** Project colours come from the server; only plain hex colours may reach a style attribute. */
export const safeColor = (c: string | null | undefined): string =>
  c && /^#[0-9a-fA-F]{3,8}$/.test(c) ? c : "#999999";

export const label = (i: Item): string => (i.task ? `${i.project} · ${i.task}` : i.project);

/** Elapsed time since `start` as h:mm:ss. */
export function elapsed(start: string, now: number = Date.now()): string {
  const secs = Math.max(0, Math.floor((now - Date.parse(start)) / 1000));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

/** Subsequence fuzzy match; higher is better, -1 means no match. */
export function score(query: string, text: string): number {
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

/** Items matching `query` (by client, project and task), best first. */
export function search(query: string, items: Item[]): Item[] {
  return items
    .map((i) => ({ i, s: score(query, `${i.client ?? ""} ${label(i)}`) }))
    .filter((x) => x.s >= 0)
    .sort((a, b) => b.s - a.s)
    .map((x) => x.i);
}

export function itemHtml(i: Item, idx: number, sel: boolean): string {
  return `
    <li class="item ${sel ? "sel" : ""}" data-idx="${idx}">
      <span class="dot" style="background:${safeColor(i.color)}"></span>
      <span class="grow"><b>${esc(i.project)}</b>${i.task ? ` <span class="task">· ${esc(i.task)}</span>` : ""}</span>
      <span class="client">${esc(i.client ?? "Internal")}</span>
    </li>`;
}

export const hhmm = (iso: string): string =>
  new Date(iso).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });

/** Duration as "7h 42m" or "12m". */
export function duration(secs: number): string {
  const m = Math.max(0, Math.round(secs / 60));
  const h = Math.floor(m / 60);
  return h ? `${h}h ${String(m % 60).padStart(2, "0")}m` : `${m}m`;
}

export type DayRow =
  | { kind: "entry"; entry: EntryView; secs: number }
  | { kind: "gap"; start: string; end: string; secs: number };

/** Gaps shorter than this between two entries are not shown. */
export const MIN_GAP_SECS = 5 * 60;

/** The day's entries with untracked gaps between them (W8: "blocks and gaps"). */
export function dayRows(entries: EntryView[], now: number = Date.now()): DayRow[] {
  const rows: DayRow[] = [];
  let prevEnd: number | null = null;
  let prevEndIso = "";
  for (const e of entries) {
    const start = Date.parse(e.start);
    const end = e.end ? Date.parse(e.end) : now;
    if (prevEnd !== null && start - prevEnd >= MIN_GAP_SECS * 1000) {
      rows.push({ kind: "gap", start: prevEndIso, end: e.start, secs: (start - prevEnd) / 1000 });
    }
    rows.push({ kind: "entry", entry: e, secs: Math.max(0, (end - start) / 1000) });
    if (prevEnd === null || end > prevEnd) {
      prevEnd = end;
      prevEndIso = e.end ?? new Date(now).toISOString();
    }
  }
  return rows;
}

/** Screenshots taken while this entry ran. */
export function capturesIn(e: EntryView, caps: CaptureView[], now: number = Date.now()): CaptureView[] {
  const start = Date.parse(e.start);
  const end = e.end ? Date.parse(e.end) : now;
  return caps.filter((c) => {
    const t = Date.parse(c.taken_at);
    return t >= start && t < end;
  });
}

const pad = (n: number) => String(n).padStart(2, "0");

/** Local "HH:MM" for an `<input type="time">`. */
export function localHhmm(iso: string): string {
  const d = new Date(iso);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Local "HH:MM" on the same local day as `baseIso`, as UTC without milliseconds
 *  (the format solidtime expects). Null if `hhmm` is not a valid time. */
export function atLocalTime(baseIso: string, hhmm: string): string | null {
  const m = /^([01]\d|2[0-3]):([0-5]\d)$/.exec(hhmm.trim());
  if (!m) return null;
  const d = new Date(baseIso);
  d.setHours(Number(m[1]), Number(m[2]), 0, 0);
  return d.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/** A screenshot that points to another project than the block it falls in. */
export function mismatch(c: CaptureView, e: EntryView): boolean {
  return c.kind === "project" && !!c.item && c.item.project_id !== e.item?.project_id;
}

/** How auto-tracking decided the project for a screenshot. */
export function decidedBy(c: CaptureView): string {
  switch (c.source) {
    case "ai":
      return c.confidence == null ? "AI" : `AI · ${Math.round(c.confidence * 100)}% sure`;
    case "keywords":
      return "project keywords";
    case "cache":
      return "learned earlier";
    case "pending":
      return "not recognised yet";
    default:
      return c.source;
  }
}

/** The company server set at build time (VITE_TIMEPILOT_SERVER_URL), else local development. */
export const defaultServer = (configured: string | undefined): string =>
  (configured ?? "").trim().replace(/\/+$/, "") || "http://localhost:8000";

/** What the auto-tracking window says after "I agree", and whether it can close: it
 * stays open only when something still needs the user (AI missing, Screen Recording). */
export function afterTurnOn(v: { server_ready: boolean; permission: string }): {
  text: string;
  error: boolean;
  close: boolean;
} {
  if (!v.server_ready)
    return {
      text: "Auto-tracking is on, but AI is not configured on the server yet (keywords only).",
      error: true,
      close: false,
    };
  if (v.permission === "denied")
    return {
      text: "Auto-tracking is on. Allow Screen Recording below so it can use screenshots.",
      error: false,
      close: false,
    };
  return { text: "Auto-tracking is on.", error: false, close: true };
}

/** How the consent screen names the AI the server uses: "an AI (Google Gemini)". */
export function aiName(provider: string | null | undefined): string {
  const names: Record<string, string> = { gemini: "Google Gemini", anthropic: "Anthropic Claude" };
  const name = names[(provider ?? "").trim().toLowerCase()];
  return name ? `an AI (${name})` : "an AI";
}

/** "1 minute", "5 minutes". */
export const plural = (n: number, word: string): string => `${n} ${word}${n === 1 ? "" : "s"}`;
