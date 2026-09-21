import { retestDeltaLabel } from "./retest-delta";

const run = (id: string, ms: number, baseline = false) => ({
  QueryId: id,
  TimeMs: ms,
  IsBaseline: baseline,
});

describe("retestDeltaLabel", () => {
  it("stays empty when there is nothing to compare", () => {
    expect(retestDeltaLabel([])).toBe("");
    expect(retestDeltaLabel([run("a", 1000, true)])).toBe("");
  });

  it("explains when no new execution exists yet", () => {
    const label = retestDeltaLabel([
      run("a", 1000, true),
      run("a", 1000, true),
    ]);

    expect(label).toContain("尚无新的执行记录");
  });

  it("compares the baseline against the latest run", () => {
    expect(retestDeltaLabel([run("a", 30_650, true), run("b", 22_100)])).toBe(
      "诊断那次 30.65s → 最近一次 22.10s（↓ 28%）",
    );

    expect(retestDeltaLabel([run("a", 1000, true), run("b", 1500)])).toBe(
      "诊断那次 1.00s → 最近一次 1.50s（↑ 50%）",
    );
  });
});
