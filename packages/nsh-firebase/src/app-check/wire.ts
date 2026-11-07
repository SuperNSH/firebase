// The App Check exchange wire shapes: the debug-token exchange URL,
// request, and the {token, ttl} reply parse, per the REST reference
// (firebaseappcheck.googleapis.com/v1, AppCheckToken = {token, ttl})
// and the client SDKs' exchange (debug_token in the body, ttl a Protobuf
// Duration like "1800s"). The exchange carries only Content-Type:
// X-Firebase-AppCheck is the header CALLS to protected backends attach
// afterwards, not this exchange.

import { joinUrl } from "../core";
import { invalidResponse } from "./error";

// The REST reference's service endpoint (the client SDKs use the
// content- frontend variant; both serve the same paths).
export const APP_CHECK_BASE = "https://firebaseappcheck.googleapis.com/v1";

const EXCHANGE_DEBUG_TOKEN = "exchangeDebugToken";

// The exchange URL:
// {base}/projects/{project}/apps/{appId}:exchangeDebugToken?key={apiKey}
// — the API key in the query as the client SDKs send it, the ids raw
// (":" is legal in a path segment).
export function exchangeDebugTokenUrl(
  base: string,
  projectId: string,
  appId: string,
  apiKey: string,
): string {
  return `${joinUrl(base, `projects/${projectId}/apps/${appId}:${EXCHANGE_DEBUG_TOKEN}`)}?key=${apiKey}`;
}

// The exchange request body: {"debug_token": ...} — the snake_case
// spelling every client SDK sends; the API reference's camelCase
// debugToken is the same proto field.
export function exchangeDebugTokenBody(debugToken: string): string {
  return JSON.stringify({ debug_token: debugToken });
}

// One success reply reduced to what the cache needs.
export interface TokenReply {
  token: string;
  ttlMs: number;
}

export function parseTokenReply(text: string): TokenReply {
  let reply: unknown;
  try {
    reply = JSON.parse(text);
  } catch {
    throw invalidResponse("the reply is not JSON");
  }
  const record = reply as { token?: unknown; ttl?: unknown } | null;
  if (typeof record?.token !== "string") {
    throw invalidResponse("the reply has no token");
  }
  if (typeof record.ttl !== "string") {
    throw invalidResponse('the reply has no ttl (a Protobuf Duration like "1800s")');
  }
  const ttlMs = parseTtlMs(record.ttl);
  if (ttlMs === null) {
    throw invalidResponse('the reply has no usable ttl (a Protobuf Duration like "1800s")');
  }
  return { token: record.token, ttlMs };
}

// Parses a Protobuf Duration ("1800s", "0s", fractional allowed) to
// milliseconds, the way the client SDKs do (^([\d.]+)(s)$).
export function parseTtlMs(ttl: string): number | null {
  if (!ttl.endsWith("s")) return null;
  const secs = ttl.slice(0, -1);
  if (secs === "" || !/^[0-9.]+$/.test(secs)) return null;
  const value = Number(secs);
  if (!Number.isFinite(value) || value < 0) return null;
  return value * 1000;
}
