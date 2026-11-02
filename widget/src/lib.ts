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
  running: { item: Item | null; description: string; start: string } | null;
  recent: Item[];
  items: Item[];
  pending: number;
  offline: boolean;
  error: string | null;
}

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
  new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
