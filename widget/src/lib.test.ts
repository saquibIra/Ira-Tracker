import { describe, expect, it } from "vitest";
import {
  afterTurnOn,
  atLocalTime,
  capturesIn,
  dayRows,
  decidedBy,
  aiName,
  defaultServer,
  duration,
  elapsed,
  esc,
  itemHtml,
  label,
  localHhmm,
  mismatch,
  plural,
  safeColor,
  score,
  search,
  type CaptureView,
  type EntryView,
  type Item,
} from "./lib";

const item = (project: string, task: string | null = null, client: string | null = null): Item => ({
  project_id: project,
  task_id: task,
  project,
  task,
  client,
  color: "#42a5f5",
});

describe("esc", () => {
  it("neutralises HTML special characters", () => {
    expect(esc(`<img src=x onerror="alert('x')">&`)).toBe(
      "&#60;img src=x onerror=&#34;alert(&#39;x&#39;)&#34;&#62;&#38;",
    );
  });
});

describe("safeColor", () => {
  it("accepts hex colours", () => {
    expect(safeColor("#ef5350")).toBe("#ef5350");
    expect(safeColor("#FFF")).toBe("#FFF");
  });
  it("rejects anything that could inject CSS", () => {
    expect(safeColor("red;background:url(https://evil)")).toBe("#999999");
    expect(safeColor('#fff" onmouseover="x')).toBe("#999999");
    expect(safeColor(null)).toBe("#999999");
  });
});

describe("itemHtml", () => {
  it("escapes server-provided names", () => {
    const html = itemHtml({ ...item("<b>x</b>", "<i>t</i>", "<s>c</s>"), color: "x;y" }, 0, true);
    expect(html).not.toContain("<b>x</b>");
    expect(html).not.toContain("<i>t</i>");
    expect(html).toContain("#999999");
    expect(html).toContain('class="item sel"');
  });
});

describe("elapsed", () => {
  it("formats h:mm:ss", () => {
    const start = "2026-11-02T09:00:00Z";
    expect(elapsed(start, Date.parse("2026-11-02T10:05:09Z"))).toBe("1:05:09");
  });
  it("never goes negative", () => {
    expect(elapsed("2026-11-02T09:00:00Z", Date.parse("2026-11-02T08:00:00Z"))).toBe("0:00:00");
  });
});

describe("score / search", () => {
  const items = [
    item("Acme – POS Revamp", "Backend", "Acme Retail"),
    item("Acme – POS Revamp", "Frontend", "Acme Retail"),
    item("Globex – Patient Portal", "Testing", "Globex Health"),
    item("Internal", "Meetings"),
  ];

  it("matches substrings before fuzzy matches", () => {
    expect(score("pos", "Acme – POS Revamp")).toBeGreaterThan(score("prv", "Acme – POS Revamp"));
  });

  it("returns -1 when letters are missing", () => {
    expect(score("xyz", "Acme")).toBe(-1);
  });

  it("finds project and task by abbreviation", () => {
    expect(label(search("pos back", items)[0])).toBe("Acme – POS Revamp · Backend");
  });

  it("matches on client name", () => {
    expect(search("globex", items).map((i) => i.project)).toEqual(["Globex – Patient Portal"]);
  });

  it("returns nothing for no match", () => {
    expect(search("zzz", items)).toEqual([]);
  });
});

const entry = (start: string, end: string | null, extra: Partial<EntryView> = {}): EntryView => ({
  local_id: start,
  start,
  end,
  item: null,
  description: "",
  auto: true,
  needs_review: false,
  running: end === null,
  error: null,
  ...extra,
});

describe("duration", () => {
  it("formats hours and minutes", () => {
    expect(duration(0)).toBe("0m");
    expect(duration(59 * 60)).toBe("59m");
    expect(duration(7 * 3600 + 42 * 60)).toBe("7h 42m");
    expect(duration(3600 + 5 * 60)).toBe("1h 05m");
  });
});

describe("dayRows", () => {
  it("inserts gaps of five minutes or more", () => {
    const rows = dayRows([
      entry("2026-11-19T09:00:00Z", "2026-11-19T10:00:00Z"),
      entry("2026-11-19T10:03:00Z", "2026-11-19T11:00:00Z"),
      entry("2026-11-19T11:30:00Z", "2026-11-19T12:00:00Z"),
    ]);
    expect(rows.map((r) => r.kind)).toEqual(["entry", "entry", "gap", "entry"]);
    const gap = rows[2];
    expect(gap.kind === "gap" && gap.secs).toBe(1800);
  });

  it("counts a running entry up to now", () => {
    const now = Date.parse("2026-11-19T09:30:00Z");
    const rows = dayRows([entry("2026-11-19T09:00:00Z", null)], now);
    expect(rows[0].secs).toBe(1800);
  });

  it("does not report a gap inside an earlier long entry", () => {
    const rows = dayRows([
      entry("2026-11-19T09:00:00Z", "2026-11-19T12:00:00Z"),
      entry("2026-11-19T10:00:00Z", "2026-11-19T10:30:00Z"),
      entry("2026-11-19T11:00:00Z", "2026-11-19T11:30:00Z"),
    ]);
    expect(rows.filter((r) => r.kind === "gap")).toHaveLength(0);
  });
});

const shot = (taken_at: string, project: string | null, extra: Partial<CaptureView> = {}): CaptureView => ({
  id: taken_at,
  taken_at,
  app: "Code",
  title: "index.ts — mmcg-platform",
  kind: project ? "project" : "neutral",
  item: project ? item(project) : null,
  note: "",
  confidence: null,
  source: "keywords",
  thumb: null,
  ...extra,
});

describe("capturesIn", () => {
  it("keeps screenshots taken while the entry ran", () => {
    const e = entry("2026-11-19T09:00:00Z", "2026-11-19T10:00:00Z");
    const caps = [
      shot("2026-11-19T08:59:00Z", "A"),
      shot("2026-11-19T09:00:00Z", "A"),
      shot("2026-11-19T09:59:59Z", "A"),
      shot("2026-11-19T10:00:00Z", "A"),
    ];
    expect(capturesIn(e, caps).map((c) => c.taken_at)).toEqual(["2026-11-19T09:00:00Z", "2026-11-19T09:59:59Z"]);
  });
  it("uses now for the running entry", () => {
    const e = entry("2026-11-19T09:00:00Z", null);
    const now = Date.parse("2026-11-19T09:30:00Z");
    expect(capturesIn(e, [shot("2026-11-19T09:20:00Z", "A"), shot("2026-11-19T09:40:00Z", "A")], now)).toHaveLength(1);
  });
});

describe("local times", () => {
  it("round-trips HH:MM on the entry's day as UTC without milliseconds", () => {
    const base = "2026-11-19T09:00:00Z";
    const iso = atLocalTime(base, localHhmm(base));
    expect(iso).toBe(base);
    expect(atLocalTime(base, "25:00")).toBeNull();
    expect(atLocalTime(base, "9:5")).toBeNull();
    expect(atLocalTime(base, "10:30")).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:00Z$/);
  });
});

describe("screenshot labels", () => {
  it("flags screenshots that show another project", () => {
    const e = { ...entry("2026-11-19T09:00:00Z", null), item: item("A") };
    expect(mismatch(shot("2026-11-19T09:10:00Z", "B"), e)).toBe(true);
    expect(mismatch(shot("2026-11-19T09:10:00Z", "A"), e)).toBe(false);
    expect(mismatch(shot("2026-11-19T09:10:00Z", null), e)).toBe(false);
  });
  it("explains how the project was decided", () => {
    expect(decidedBy(shot("t", "A", { source: "ai", confidence: 0.923 }))).toBe("AI · 92% sure");
    expect(decidedBy(shot("t", "A"))).toBe("project keywords");
    expect(plural(1, "minute")).toBe("1 minute");
    expect(plural(5, "minute")).toBe("5 minutes");
  });
});

describe("production defaults", () => {
  it("uses the server baked in at build time, else local development", () => {
    expect(defaultServer("https://time.example.com/")).toBe("https://time.example.com");
    expect(defaultServer(undefined)).toBe("http://localhost:8000");
    expect(defaultServer("  ")).toBe("http://localhost:8000");
  });
  it("names the server's AI on the consent screen", () => {
    expect(aiName("gemini")).toBe("an AI (Google Gemini)");
    expect(aiName(" Anthropic ")).toBe("an AI (Anthropic Claude)");
    expect(aiName("")).toBe("an AI");
    expect(aiName(undefined)).toBe("an AI");
    expect(aiName("<b>x</b>")).toBe("an AI");
  });
});

describe("afterTurnOn", () => {
  it("closes the window when everything is ready", () => {
    expect(afterTurnOn({ server_ready: true, permission: "granted" })).toEqual({
      text: "Auto-tracking is on.",
      error: false,
      close: true,
    });
    // Systems without screenshots have nothing left to ask for either.
    expect(afterTurnOn({ server_ready: true, permission: "unsupported" }).close).toBe(true);
  });
  it("stays open while Screen Recording is still needed", () => {
    const r = afterTurnOn({ server_ready: true, permission: "denied" });
    expect(r.close).toBe(false);
    expect(r.error).toBe(false);
    expect(r.text).toContain("Screen Recording");
  });
  it("stays open with a warning when the server has no AI", () => {
    const r = afterTurnOn({ server_ready: false, permission: "granted" });
    expect(r).toMatchObject({ error: true, close: false });
    expect(r.text).toContain("not configured");
  });
});
