import { describe, expect, it } from "vitest";
import {
  amount,
  type Group,
  highlights,
  kind,
  label,
  lead,
  machine,
  margin,
  measured,
  metrics,
  ours,
  plotted,
  position,
  ranked,
  SECTIONS,
  SELF,
  type Section,
  scale,
  single,
  subjects,
  tables,
  tick,
  tools,
  width,
} from "./bench";

const group = (better: Group["better"]): Group => ({
  id: "fir",
  subject: "FIR",
  title: "FIR",
  unit: "Msps",
  better,
  results: [
    { tool: "a", version: "1", value: 50 },
    { tool: SELF, version: "1", value: 200 },
    { tool: "b", version: "1", value: 100 },
  ],
});

describe("ranked", () => {
  it("puts the best result first", () => {
    expect(ranked(group("higher")).map((entry) => entry.value)).toEqual([200, 100, 50]);
    expect(ranked(group("lower")).map((entry) => entry.value)).toEqual([50, 100, 200]);
  });

  it("lists us first on a tie", () => {
    const results = [
      { tool: "Dire Wolf", version: "1", value: 81.25 },
      { tool: SELF, version: "1", value: 81.25 },
    ];
    expect(ranked({ ...group("higher"), results })[0]?.tool).toBe(SELF);
  });
});

describe("width", () => {
  it("scales bars to the largest value", () => {
    const fir = group("higher");
    expect(fir.results.map((entry) => width(entry, fir))).toEqual([0.25, 1, 0.5]);
  });
});

describe("amount", () => {
  it("keeps three significant figures", () => {
    expect(amount(1234.5)).toBe("1,235");
    expect(amount(12.34)).toBe("12.3");
    expect(amount(1.234)).toBe("1.23");
  });

  it("shows counts without decimals", () => {
    expect(amount(6)).toBe("6");
    expect(amount(21)).toBe("21");
  });
});

describe("machine", () => {
  it("skips missing parts", () => {
    expect(machine({ machine: { cpu: "M4", os: "", date: "2026-10-01" }, groups: [] })).toBe(
      "M4 · 2026-10-01",
    );
  });
});

describe("lead", () => {
  it("compares us with the best other tool", () => {
    expect(lead(group("higher"))).toBe(2);
    expect(lead(group("lower"))).toBe(0.25);
  });

  it("uses our weakest variant", () => {
    const results = [
      { tool: `${SELF} app`, version: "1", value: 40 },
      { tool: `${SELF} headless`, version: "1", value: 10 },
      { tool: "other", version: "1", value: 20 },
    ];
    expect(lead({ ...group("lower"), results })).toBe(0.5);
  });

  it("is zero without a rival", () => {
    expect(lead({ ...group("higher"), results: [{ tool: SELF, version: "1", value: 1 }] })).toBe(0);
  });
});

describe("highlights", () => {
  const section = (groups: Group[]): Section => ({
    id: "dsp",
    title: "DSP",
    command: "",
    form: "rows",
    span: "row",
    suite: { machine: { cpu: "", os: "", date: "" }, groups },
  });

  it("picks our biggest lead per section", () => {
    const wide = {
      ...group("higher"),
      id: "wide",
      results: [...group("higher").results, { tool: "c", version: "1", value: 20 }],
    };
    const narrow = {
      ...group("higher"),
      id: "narrow",
      results: [
        { tool: SELF, version: "1", value: 110 },
        { tool: "a", version: "1", value: 100 },
      ],
    };
    expect(highlights([section([narrow, wide])]).map((found) => found.id)).toEqual(["wide"]);
  });

  it("skips sections where we trail", () => {
    expect(highlights([section([group("lower")]), section([])])).toEqual([]);
  });
});

describe("published data", () => {
  it("names a subject for every group", () => {
    for (const section of SECTIONS) {
      for (const result of section.suite.groups) {
        expect(result.subject.length).toBeGreaterThan(0);
        expect(result.title).toContain(result.subject);
      }
    }
  });

  it("has our result in every group", () => {
    for (const section of measured(SECTIONS)) {
      for (const result of section.suite.groups) {
        expect(result.results.some(ours)).toBe(true);
      }
    }
  });
});

const metric = (subject: string, unit: string, values: Record<string, number>): Group => ({
  id: `${subject}-${unit}`,
  subject,
  title: `${subject} ${unit === "% decoded" ? "in noise" : "speed"}`,
  unit,
  better: "higher",
  results: Object.entries(values).map(([tool, value]) => ({ tool, version: "1", value })),
});

describe("tables", () => {
  it("splits by unit and lists our tools first", () => {
    const groups = [
      metric("1 receiver", "MiB", { other: 5, [`${SELF} app`]: 9 }),
      metric("1 receiver", "% core", { [`${SELF} headless`]: 1 }),
      metric("4 receivers", "MiB", { third: 2, other: 6 }),
    ];
    const split = tables(groups);
    expect(split.map((table) => table.unit)).toEqual(["MiB", "% core"]);
    expect(split[0]?.tools).toEqual([`${SELF} app`, "other", "third"]);
    expect(split[0]?.rows.map((row) => row.subject)).toEqual(["1 receiver", "4 receivers"]);
    expect(subjects(groups)).toEqual(["1 receiver", "4 receivers"]);
    expect(tools(groups)[0]).toBe(`${SELF} app`);
  });
});

describe("single", () => {
  it("is false when a group has two of ours", () => {
    expect(single([group("higher")])).toBe(true);
    expect(single([metric("a", "MiB", { [`${SELF} app`]: 1, [`${SELF} headless`]: 2 })])).toBe(
      false,
    );
  });
});

describe("kind", () => {
  it("tells shares, counts and rates apart", () => {
    const groups = [
      metric("a", "% decoded", { [SELF]: 50.5 }),
      metric("a", "msgs", { [SELF]: 3, other: 4 }),
      metric("a", "× realtime", { [SELF]: 3.5 }),
    ];
    expect(kind("% decoded", groups)).toBe("share");
    expect(kind("msgs", groups)).toBe("count");
    expect(kind("× realtime", groups)).toBe("rate");
  });
});

describe("margin", () => {
  it("states rates as a factor", () => {
    expect(margin(group("higher"), "rate")).toBe("2.0× faster");
    expect(margin(metric("a", "× realtime", { [SELF]: 10, other: 250 }), "rate")).toBe(
      "25× slower",
    );
    expect(margin(metric("a", "× realtime", { [SELF]: 100, other: 99 }), "rate")).toBe("even");
  });

  it("states shares in points", () => {
    expect(margin(metric("a", "% decoded", { [SELF]: 67.75, other: 56.25 }), "share")).toBe(
      "+11.5 pts",
    );
    expect(margin(metric("a", "% decoded", { [SELF]: 50, other: 52 }), "share")).toBe(
      "\u22122.0 pts",
    );
    expect(margin(metric("a", "% decoded", { [SELF]: 81.25, other: 81.25 }), "share")).toBe("even");
  });

  it("states counts as a difference", () => {
    expect(margin(metric("a", "msgs", { [SELF]: 21, other: 17 }), "count")).toBe("+4");
    expect(margin(metric("a", "msgs", { [SELF]: 5, other: 6 }), "count")).toBe("\u22121");
    expect(margin(metric("a", "msgs", { [SELF]: 2, other: 2 }), "count")).toBe("even");
  });

  it("is empty without a rival", () => {
    expect(margin(metric("a", "msgs", { [SELF]: 21 }), "count")).toBe("");
  });
});

describe("label", () => {
  it("drops the subject from the title", () => {
    expect(label(metric("ADS-B", "% decoded", {}))).toBe("In noise");
    expect(label({ ...metric("4 receivers", "MiB", {}), title: "4 receivers, memory" })).toBe(
      "Memory",
    );
  });
});

describe("metrics", () => {
  it("puts shares before rates and counts", () => {
    const groups = [
      metric("a", "msgs", { [SELF]: 3 }),
      metric("a", "× realtime", { [SELF]: 3.5 }),
      metric("a", "% decoded", { [SELF]: 50.5 }),
    ];
    expect(metrics(groups)).toEqual(["% decoded", "× realtime", "msgs"]);
  });
});

describe("scale", () => {
  it("puts shares on a fixed percent axis", () => {
    const axis = scale("% decoded", [metric("a", "% decoded", { [SELF]: 67.5 })]);
    expect(axis).toEqual({ log: false, min: 0, max: 100, ticks: [0, 50, 100] });
    expect(position(67.5, axis)).toBe(0.675);
  });

  it("spans whole decades for rates", () => {
    const groups = [
      metric("a", "× realtime", { [SELF]: 33.5, other: 19_142 }),
      metric("b", "× realtime", { [SELF]: 468 }),
    ];
    const axis = scale("× realtime", groups);
    expect(axis.ticks).toEqual([10, 100, 1000, 10_000, 100_000]);
    expect(position(1000, axis)).toBe(0.5);
    expect(position(1, axis)).toBe(0);
  });
});

describe("tick", () => {
  it("shortens thousands", () => {
    expect([10, 1000, 100_000, 2_000_000].map(tick)).toEqual(["10", "1k", "100k", "2M"]);
  });
});

describe("plotted", () => {
  it("draws ours last so it sits on top", () => {
    const order = plotted(metric("a", "msgs", { [SELF]: 1, other: 2, third: 3 }));
    expect(order.at(-1)?.tool).toBe(SELF);
  });
});
