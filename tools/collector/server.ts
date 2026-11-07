// The local HTTP sink for the pure tier's analytics and perf transports.
// It asserts what was actually sent, so no Firebase account is needed:
//   bun tools/collector/server.ts [--out docs/evidence/perfmon/collector.jsonl] [--port 18742]
// Routes:
//   POST /collect                     append each JSON body as one line
//   GET  /assert?contains=<s>&count=<n>  {"ok":bool,"count":n,"matches":k}
//   GET|POST /slow?ms=1500            reply after ms (the demo's slow route)
//   GET  /health                      200
// `ok` needs both asks: every `count` is a minimum-lines ask, and a
// `contains` ask needs at least one line with it; `matches` counts them.
// The sink logic (`appendBody`, `assertLines`) is exported for unit tests,
// which run without the server (`bun test tools/collector`).

import { appendFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";

export const DEFAULT_OUT = "docs/evidence/perfmon/collector.jsonl";
export const DEFAULT_PORT = 18742;

export interface AssertAsk {
  contains?: string | null;
  count?: number | null;
}

export interface AssertAnswer {
  ok: boolean;
  count: number | null;
  matches: number;
}

// One line per JSON body; blank lines (a trailing newline) change nothing.
export function appendBody(lines: string[], body: string): number {
  const fresh = body.split("\n").map((line) => line.trim()).filter((line) => line.length > 0);
  lines.push(...fresh);
  return fresh.length;
}

export function assertLines(lines: string[], ask: AssertAsk): AssertAnswer {
  const matches = ask.contains ? lines.filter((line) => line.includes(ask.contains!)).length : 0;
  const containsOk = ask.contains ? matches >= 1 : true;
  const countOk = typeof ask.count === "number" ? lines.length >= ask.count : true;
  return { ok: containsOk && countOk, count: typeof ask.count === "number" ? ask.count : null, matches };
}

function intArgument(url: URL, name: string): number | null {
  const raw = url.searchParams.get(name);
  if (raw === null) return null;
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}

function start(out: string, port: number): void {
  const lines: string[] = [];
  mkdirSync(dirname(out), { recursive: true });

  const server = Bun.serve({
    port,
    async fetch(request): Promise<Response> {
      const url = new URL(request.url);
      const route = `${request.method} ${url.pathname}`;
      if (route === "POST /collect") {
        const body = await request.text();
        const appended = appendBody(lines, body);
        appendFileSync(out, lines.slice(lines.length - appended).map((line) => `${line}\n`).join(""));
        return Response.json({ ok: true, appended, total: lines.length });
      }
      if (route === "GET /assert") {
        return Response.json(assertLines(lines, {
          contains: url.searchParams.get("contains"),
          count: intArgument(url, "count"),
        }));
      }
      if (url.pathname === "/slow") {
        const ms = intArgument(url, "ms") ?? 1500;
        await Bun.sleep(Math.max(0, ms));
        return Response.json({ ok: true, slow: ms });
      }
      if (route === "GET /health") {
        return Response.json({ ok: true });
      }
      return new Response("not found", { status: 404 });
    },
  });
  console.log(`collector listening on http://127.0.0.1:${server.port} out=${out}`);
}

if (import.meta.main) {
  const argv = process.argv.slice(2);
  const out = argv.includes("--out") ? resolve(argv[argv.indexOf("--out") + 1]) : resolve(DEFAULT_OUT);
  const portArg = argv.includes("--port") ? Number(argv[argv.indexOf("--port") + 1]) : DEFAULT_PORT;
  start(out, portArg);
}
