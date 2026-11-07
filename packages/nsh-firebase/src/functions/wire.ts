// The https.callable wire format: URLs, the request body, and the
// response/error mapping, exactly as the firebase-js-sdk functions layer
// speaks it (@firebase/functions 0.14.0 dist/esm/index.esm.js: _url,
// postJSON, callAtURL, _errorForResponse, errorCodeMap) and as the
// server side writes it (firebase-functions v1
// src/common/providers/https.ts: res.status(200).send({ result }),
// res.status(status).send({ error: httpErr.toJSON() })). Mirrors
// crates/nsh-firebase-functions/src/wire.rs name for name.

import { FirebaseError } from "../core/error";

/** The region a callable answers under when none is configured. */
export const DEFAULT_REGION = "us-central1";

/** The sandbox suite's Functions emulator (emulator/firebase.json). */
export const FUNCTIONS_EMULATOR_PORT = 15001;

/** The native handler this product routes through when a host answers. */
export const NATIVE_CALL = "firebase.functions.call";

/** The documented callable error codes, `functions/`-prefixed. */
export const OK = "functions/ok";
export const CANCELLED = "functions/cancelled";
export const UNKNOWN = "functions/unknown";
export const INVALID_ARGUMENT = "functions/invalid-argument";
export const DEADLINE_EXCEEDED = "functions/deadline-exceeded";
export const NOT_FOUND = "functions/not-found";
export const ALREADY_EXISTS = "functions/already-exists";
export const PERMISSION_DENIED = "functions/permission-denied";
export const UNAUTHENTICATED = "functions/unauthenticated";
export const RESOURCE_EXHAUSTED = "functions/resource-exhausted";
export const FAILED_PRECONDITION = "functions/failed-precondition";
export const ABORTED = "functions/aborted";
export const OUT_OF_RANGE = "functions/out-of-range";
export const UNIMPLEMENTED = "functions/unimplemented";
export const INTERNAL = "functions/internal";
export const UNAVAILABLE = "functions/unavailable";
export const DATA_LOSS = "functions/data-loss";

/** One emulator endpoint: host with a default, port required. */
export interface FunctionsEmulatorEndpoint {
  /** Missing: 127.0.0.1, the sandbox's. */
  host?: string;
  port: number;
}

/** Where a call goes: the region, and the emulator switch that wins
 * over production (`{region}-{project}.cloudfunctions.net`). */
export interface FunctionsConfig {
  region?: string;
  emulator?: FunctionsEmulatorEndpoint | null;
}

/** The sandbox default: the suite's Functions emulator on
 * 127.0.0.1:15001 (a host override keeps the http scheme). */
export function defaultFunctionsConfig(): Required<Pick<FunctionsConfig, "region">> &
  FunctionsConfig {
  return { region: DEFAULT_REGION, emulator: { port: FUNCTIONS_EMULATOR_PORT } };
}

/** The URL of one callable: the emulator answers at
 * `{origin}/{projectId}/{region}/{name}`, production at
 * `https://{region}-{projectId}.cloudfunctions.net/{name}` (the SDK's
 * `FunctionsService._url`; custom domains are not configured here). */
export function callableUrl(
  config: FunctionsConfig,
  projectId: string,
  name: string,
): string {
  const region = config.region ?? DEFAULT_REGION;
  const emulator = config.emulator;
  if (emulator) {
    return `http://${emulator.host ?? "127.0.0.1"}:${emulator.port}/${projectId}/${region}/${name}`;
  }
  return `https://${region}-${projectId}.cloudfunctions.net/${name}`;
}

/** A failed call: the SDK's `functions/<code>` (or a `core/*` code when
 * the failure came from the native channel), the backend message with
 * its ` [<httpStatus>]` suffix, and the optional `error.details`. */
export class FunctionsError extends FirebaseError {
  readonly details: unknown;

  constructor(code: string, message: string, details?: unknown) {
    super(code, message);
    this.name = "FunctionsError";
    this.details = details ?? null;
  }
}

/** The canonical names the server writes into `error.status`, mapped to
 * the SDK's client codes (`errorCodeMap`'s reverse). */
const CLIENT_BY_CANONICAL: Record<string, string> = {
  OK: "ok",
  CANCELLED: "cancelled",
  UNKNOWN: "unknown",
  INVALID_ARGUMENT: "invalid-argument",
  DEADLINE_EXCEEDED: "deadline-exceeded",
  NOT_FOUND: "not-found",
  ALREADY_EXISTS: "already-exists",
  PERMISSION_DENIED: "permission-denied",
  UNAUTHENTICATED: "unauthenticated",
  RESOURCE_EXHAUSTED: "resource-exhausted",
  FAILED_PRECONDITION: "failed-precondition",
  ABORTED: "aborted",
  OUT_OF_RANGE: "out-of-range",
  UNIMPLEMENTED: "unimplemented",
  INTERNAL: "internal",
  UNAVAILABLE: "unavailable",
  DATA_LOSS: "data-loss",
};

/** The HTTP status fallback (`codeForHTTPStatus`, google/rpc/code.proto). */
function clientCodeForStatus(status: number): string {
  if (status >= 200 && status < 300) return "ok";
  switch (status) {
    case 0:
    case 500:
      return "internal";
    case 400:
      return "invalid-argument";
    case 401:
      return "unauthenticated";
    case 403:
      return "permission-denied";
    case 404:
      return "not-found";
    case 409:
      return "aborted";
    case 429:
      return "resource-exhausted";
    case 499:
      return "cancelled";
    case 501:
      return "unimplemented";
    case 503:
      return "unavailable";
    case 504:
      return "deadline-exceeded";
    default:
      return "unknown";
  }
}

/** The error a response carries, if any (`_errorForResponse`): the HTTP
 * status gives the defaults, then a JSON body
 * `{error: {status, message, details}}` overrides them — an unknown
 * canonical `status` is itself an internal error. `ok` means success. */
export function errorForResponse(status: number, body: unknown): FunctionsError | null {
  let code = clientCodeForStatus(status);
  let description = code;
  let details: unknown;
  const error = (body as { error?: unknown } | null)?.error;
  if (error && typeof error === "object") {
    const wire = error as { status?: unknown; message?: unknown; details?: unknown };
    const canonical = wire.status;
    if (typeof canonical === "string") {
      // An unknown canonical name in the body must not fall through to
      // the status-code default: the SDK fails it as internal.
      const mapped = CLIENT_BY_CANONICAL[canonical];
      if (!mapped) {
        return new FunctionsError(
          INTERNAL,
          `Unknown backend error status: ${canonical} [${status}]`,
        );
      }
      code = mapped;
      description = `Backend error status: ${canonical}`;
    }
    if (typeof wire.message === "string") {
      description = wire.message;
    }
    // Attached whenever the key exists, JSON null included: the SDK
    // checks `details !== undefined`, not truthiness.
    if (wire.details !== undefined) {
      details = wire.details;
    }
  }
  if (code === "ok") return null;
  return new FunctionsError(`functions/${code}`, `${description} [${status}]`, details);
}

/** The result of one callable response: 2xx reads `data`, then the
 * legacy `result` key (the SDK's `callAtURL`); a body that is not JSON
 * at all — or a falsy JSON literal, which the SDK's `!response.json`
 * treats the same — is an internal error, as is a body without either
 * key. */
export function resultFromResponse(status: number, text: string): unknown {
  let body: unknown = null;
  try {
    body = JSON.parse(text);
  } catch {
    // A body that fails to parse counts as an empty one.
  }
  const error = errorForResponse(status, body);
  if (error) throw error;
  if (!body) {
    throw new FunctionsError(INTERNAL, "Response is not valid JSON object.");
  }
  if (typeof body === "object") {
    const record = body as Record<string, unknown>;
    if ("data" in record) return record.data;
    if ("result" in record) return record.result;
  }
  throw new FunctionsError(INTERNAL, "Response is missing data field.");
}
