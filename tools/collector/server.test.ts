// Unit tests of the collector's sink logic, without the server.
// Run: bun test tools/collector
import { describe, expect, test } from "bun:test";
import { appendBody, assertLines } from "./server.ts";

describe("appendBody", () => {
  test("appends each JSON body as one line", () => {
    const lines: string[] = [];
    expect(appendBody(lines, '{"type":"trace","name":"screen"}')).toBe(1);
    expect(lines).toEqual(['{"type":"trace","name":"screen"}']);
  });

  test("a newline-separated POST body is one line per JSON value", () => {
    const lines: string[] = [];
    const appended = appendBody(lines, '{"a":1}\n{"b":2}\n');
    expect(appended).toBe(2);
    expect(lines).toEqual(['{"a":1}', '{"b":2}']);
  });

  test("blank lines change nothing", () => {
    const lines: string[] = ['{"a":1}'];
    expect(appendBody(lines, "\n  \n")).toBe(0);
    expect(lines).toEqual(['{"a":1}']);
  });
});

describe("assertLines", () => {
  const lines = ['{"type":"trace","name":"load"}', '{"type":"http","host":"api.test"}'];

  test("a contains ask needs one matching line and counts them", () => {
    expect(assertLines(lines, { contains: '"name":"load"' })).toEqual({
      ok: true,
      count: null,
      matches: 1,
    });
    expect(assertLines(lines, { contains: '"host":"other.test"' }).ok).toBe(false);
  });

  test("a count ask is a minimum-lines ask, echoed back", () => {
    expect(assertLines(lines, { count: 2 })).toEqual({ ok: true, count: 2, matches: 0 });
    expect(assertLines(lines, { count: 3 }).ok).toBe(false);
  });

  test("both asks must hold", () => {
    expect(assertLines(lines, { contains: "trace", count: 2 }).ok).toBe(true);
    expect(assertLines(lines, { contains: "trace", count: 5 }).ok).toBe(false);
    expect(assertLines(lines, { contains: "nope", count: 1 }).ok).toBe(false);
  });
});
