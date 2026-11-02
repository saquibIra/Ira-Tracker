import { describe, expect, it } from "vitest";
import { elapsed, esc, itemHtml, label, safeColor, score, search, type Item } from "./lib";

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
