/**
 * The WebAssembly boundary.
 *
 * The Rust suite proves the engine is right; this proves the interface can reach
 * it - that every method the panels call exists, and that the JSON coming back has
 * the shape `src/types.ts` promises.
 */

import { describe, expect, test } from "vitest";
import { Engine } from "../wasm/kongzilla_wasm.js";
import { loadEngine, newEngine, statIndex, view } from "./harness";

await loadEngine();

describe("registry", () => {
  test("arrives in the shape the panels expect", () => {
    const defs = JSON.parse(Engine.statDefinitions());
    expect(defs.length).toBeGreaterThanOrEqual(29);
    const seen = new Set<number>();
    for (const def of defs as Array<{
      index: number;
      key: string;
      label: string;
      block: string;
    }>) {
      // The list arrives in the order the ladder shows them, which is not the
      // order they are numbered in: an index is a place in a saved link and
      // may never move, so a rung added between two others is numbered last
      // and placed where it reads. What must hold is that every statistic is
      // here exactly once.
      expect(seen.has(def.index)).toBe(false);
      seen.add(def.index);
      expect(def.index).toBeLessThan(defs.length);
      expect(def.key).toBeTruthy();
      expect(def.label).toBeTruthy();
      expect(["made", "draw", "combination"]).toContain(def.block);
    }
    expect(seen.size).toBe(defs.length);

    // And the blocks are whole: a reader sees three runs, not three shuffled
    // together.
    const blocks = (defs as Array<{ block: string }>).map((def) => def.block);
    const runs = blocks.filter((block, at) => block !== blocks[at - 1]);
    expect(runs).toEqual(["made", "draw", "combination"]);
  });

  test("ships block headings, class labels and rankings", () => {
    expect(JSON.parse(Engine.blockLabels()).map((entry: [string, string]) => entry[0])).toEqual([
      "made",
      "draw",
      "combination",
    ]);
    const labels = JSON.parse(Engine.classLabels());
    expect(labels).toHaveLength(169);
    expect(labels[0]).toBe("AA");
    expect(JSON.parse(Engine.rankings()).length).toBeGreaterThan(0);
  });
});

describe("a fresh session", () => {
  test("starts empty and preflop", async () => {
    const state = view(await newEngine());
    expect(state.board).toBe("");
    expect(state.street).toBe("preflop");
    expect(state.players).toHaveLength(2);
    expect(state.players[0].combos).toBe(0);
    expect(state.classWeights).toHaveLength(169);
    expect(state.equity).toBeNull();
  });
});

describe("the analysis loop", () => {
  test("the manual's example reaches the interface intact", async () => {
    const engine = await newEngine();
    engine.setBoard("Kc Qh Jh");
    engine.setRangeText("22+, A2s+, KJs+, AJo+");

    const state = view(engine);
    expect(state.street).toBe("flop");
    expect(state.boardCards).toEqual(["Kc", "Qh", "Jh"]);

    const row = (key: string) => state.breakdown.rows.find((candidate) => candidate.key === key)!;
    expect(row("top-pair").fraction).toBeGreaterThan(0);
    expect(row("flushdraw").fraction).toBeGreaterThan(0);

    const made = state.breakdown.rows
      .filter((candidate) => candidate.block === "made")
      .reduce((total, candidate) => total + candidate.fraction, 0);
    expect(made).toBeCloseTo(1, 9);
  });

  test("hovering re-filters the panel to one statistic", async () => {
    const engine = await newEngine();
    engine.setBoard("Kc Qh Jh");
    engine.setRangeText("22+, A2s+, KJs+, AJo+");

    const topPair = statIndex("top-pair");
    const hovered = JSON.parse(engine.breakdownWithin(topPair));
    const own = hovered.rows.find((row: { key: string }) => row.key === "top-pair");
    expect(own.fraction).toBeCloseTo(1, 9);

    const lit = engine.highlight(topPair);
    expect(lit).toHaveLength(169);
    expect(Array.from(lit).some((weight) => weight > 0)).toBe(true);
  });

  test("painting marks the range; a street filter narrows it", async () => {
    const engine = await newEngine();
    engine.setBoard("Kc Qh Jh");
    engine.setRangeText("22+, A2s+, KJs+, AJo+");
    const topPair = statIndex("top-pair");

    const wide = view(engine).breakdown.totalCombos;
    engine.clearGroups();
    engine.paintStat(topPair, "blue");
    expect(view(engine).marks[topPair]).toBe("blue");
    expect(view(engine).breakdown.totalCombos).toBe(wide);
    expect(view(engine).filtersEnabled).toBe(false);

    expect(engine.toggleStreetFilter(0)).toBe(true);
    const narrowed = view(engine);
    expect(narrowed.breakdown.totalCombos).toBeLessThan(wide);
    expect(narrowed.passFraction).toBeGreaterThan(0);
    expect(narrowed.passFraction).toBeLessThan(1);
    expect(narrowed.effectiveNotation.length).toBeGreaterThan(0);
    expect(narrowed.streetsOn[0]).toBe(true);
    expect(narrowed.streetCounts[0]).toBeCloseTo(narrowed.breakdown.totalCombos, 6);

    // Repainting one hand out of a category turns its marker into a gear: the
    // category no longer speaks for all of the hands inside it.
    const painted = view(engine)
      .marks.map((mark, index) => [mark, index] as const)
      .filter(([mark]) => mark === "blue");
    expect(painted.length).toBeGreaterThan(0);
    const cell = JSON.parse(engine.comboColours(0)) as Array<[number, string, string]>;
    expect(cell.length).toBe(6);
    engine.paintStat(statIndex("overpair"), "blue");
    engine.paintCombo(cell[0][0], "green");
    expect(engine.comboColour(cell[0][0])).toBe("green");
    expect(view(engine).marks[statIndex("overpair")]).toBe("mixed");

    expect(engine.toggleStreetFilter(0)).toBe(false);
    engine.clearFilters();
    expect(view(engine).passFraction).toBeCloseTo(1, 9);
    expect(view(engine).filtersEnabled).toBe(false);
  });

  test("a dealt hand is a seat of its own, and a narrow range is not", async () => {
    const engine = await newEngine();
    engine.setBoard("Kc 7d 2s");
    engine.setRangeText("QQ+");

    // Typing one combination is still a range: nobody has seen those cards.
    engine.setActive(1);
    engine.setRangeText("AhKh");
    expect(view(engine).hand).toBeNull();
    expect(view(engine).editable).toBe(true);

    // A hand is dealt deliberately, and then there is nothing to edit.
    engine.setRangeText("");
    expect(engine.addHand("AhKh")).toBe(true);
    // Dealing leaves the reader on the range they were working on, so reading
    // the hand means going to it.
    engine.setActive(view(engine).players.length - 1);
    const state = view(engine);
    expect(state.hand).toBe("AhKh");
    expect(state.editable).toBe(false);
    expect(state.dealt.sort()).toEqual(["Ah", "Kh"]);
    expect(state.equity?.exact).toBe(true);
    const sum = state.equity!.players[0].equity + state.equity!.players[1].equity;
    expect(sum).toBeCloseTo(1, 6);
  });

  test("dead cards take cards out of the deck and nothing more", async () => {
    const engine = await newEngine();
    engine.setBoard("Kc 7d 2s");
    engine.setRangeText("QQ+, AKs");
    engine.setActive(1);
    engine.setRangeText("JJ");
    engine.setActive(0);
    const before = view(engine).liveCombos;

    engine.toggleDead("Ah");
    engine.toggleDead("Kh");
    const after = view(engine);
    expect(after.liveCombos).toBeLessThan(before);
    // Two of them used to be read as a hand. They are not: the seats say what
    // is being measured, and these cards say only that nobody holds them.
    expect(after.hand).toBeNull();
    expect(after.equitySeats).toEqual([0, 1]);
  });
});

describe("editing", () => {
  test("the slider and the matrix edit the same range", async () => {
    const engine = await newEngine();
    engine.setTopPercent(10);
    expect(view(engine).players[0].combos).toBeGreaterThan(0);

    engine.setClassWeight(168, 0.5); // 22, the bottom-right cell
    expect(view(engine).classWeights[168]).toBeCloseTo(0.5, 6);

    engine.clearRange();
    expect(view(engine).players[0].combos).toBe(0);
  });

  test("seats are independent", async () => {
    const engine = await newEngine();
    engine.setRangeText("AA");
    engine.setActive(1);
    engine.setRangeText("KK");
    expect(view(engine).players[0].notation).toBe("AA");
    expect(view(engine).players[1].notation).toBe("KK");
  });
});

describe("persistence", () => {
  test("a session survives a round trip", async () => {
    const engine = await newEngine();
    engine.setBoard("Kc Qh Jh");
    engine.setRangeText("22+, AQs+");
    engine.paintStat(statIndex("top-pair"), "green");
    engine.toggleStreetFilter(0);
    engine.setMode("cumulative");
    engine.setOneCardBackdoorFlushdraw(true);
    const before = view(engine);

    const restored = await newEngine();
    restored.restore(engine.snapshot());
    expect(view(restored)).toEqual(before);
  });

  test("bad input is reported rather than swallowed", async () => {
    const engine = await newEngine();
    expect(() => engine.setBoard("Zz Qh Jh")).toThrow();
    expect(() => engine.setRangeText("not a range")).toThrow();
    engine.setBoard("Kc Qh Jh");
    expect(view(engine).street).toBe("flop");
  });
});
