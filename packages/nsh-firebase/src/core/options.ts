// App options and the emulator switches, mirroring
// crates/nsh-firebase-core/src/options.rs.

import { CODES, FirebaseError } from "./error";

export interface EmulatorEndpoint {
  /** Missing: 127.0.0.1, the sandbox's. */
  host?: string;
  port: number;
}

export interface EmulatorOptions {
  auth?: EmulatorEndpoint;
  database?: EmulatorEndpoint;
  storage?: EmulatorEndpoint;
}

export interface FirebaseOptions {
  apiKey: string;
  appId: string;
  projectId: string;
  databaseURL?: string;
  authURL?: string;
  /** The emulator's bucket name, without a gs:// prefix. */
  storageBucket?: string;
  emulator?: EmulatorOptions;
}

// The sandbox defaults: demo-nsh needs no account and no credentials;
// every value here is fake.
export function demoOptions(): FirebaseOptions {
  return {
    apiKey: "demo-nsh-fake-api-key",
    appId: "1:000000000000:web:demo-nsh",
    projectId: "demo-nsh",
    databaseURL: "http://127.0.0.1:19000",
    authURL: "http://127.0.0.1:19099",
    storageBucket: "demo-nsh.firebasestorage.app",
    emulator: {
      auth: { host: "127.0.0.1", port: 19099 },
      database: { host: "127.0.0.1", port: 19000 },
      storage: { host: "127.0.0.1", port: 19199 },
    },
  };
}

function endpointUrl(endpoint: EmulatorEndpoint): string {
  return `http://${endpoint.host ?? "127.0.0.1"}:${endpoint.port}`;
}

// The emulator switch wins over the real URL field; without a switch the
// configured URL stands.
export function authBaseUrl(options: FirebaseOptions): string | null {
  if (options.emulator?.auth) return endpointUrl(options.emulator.auth);
  return options.authURL ?? null;
}

export function databaseBaseUrl(options: FirebaseOptions): string | null {
  if (options.emulator?.database) return endpointUrl(options.emulator.database);
  return options.databaseURL ?? null;
}

export function storageBaseUrl(options: FirebaseOptions): string | null {
  if (options.emulator?.storage) return endpointUrl(options.emulator.storage);
  // The REST host is unprefixed; the bucket goes in the path.
  const bucket = options.storageBucket;
  return bucket ? `https://firebasestorage.googleapis.com/v0/b/${bucket}/o` : null;
}

// A project id is the one option no call can do without.
export function validateOptions(options: FirebaseOptions): FirebaseError | null {
  if (!options.projectId) {
    return new FirebaseError(CODES.invalidArgument, "FirebaseOptions.projectId is empty");
  }
  return null;
}

// Reads the options from a config document: apiKey, appId, projectId,
// databaseUrl, authUrl, storageBucket and an optional emulator block with
// per-product {host, port} endpoints. Mirrors from_config in
// crates/nsh-firebase-core/src/options.rs: a wrong shape or an empty
// projectId is invalid-argument.
export function optionsFromConfig(config: unknown): FirebaseOptions {
  const object = shape(config, "the config is not a JSON object");
  const string = (key: string): string | undefined => {
    const value = object[key];
    if (value === undefined || value === null) return undefined;
    if (typeof value !== "string") {
      throw new FirebaseError(CODES.invalidArgument, `${key} is not a string`);
    }
    return value;
  };
  let emulator: Record<string, unknown> | null = null;
  if (object.emulator !== undefined && object.emulator !== null) {
    emulator = shape(object.emulator, "emulator is not an object");
  }
  const endpoint = (product: string): EmulatorEndpoint | undefined => {
    const value = emulator?.[product];
    if (value === undefined || value === null) return undefined;
    const endpoint = shape(value, `emulator.${product} is not an object`);
    const port = endpoint.port;
    // Same messages the Rust from_config throws (as_u64 into u16).
    if (typeof port !== "number" || !Number.isInteger(port) || port < 0) {
      throw new FirebaseError(
        CODES.invalidArgument,
        `emulator.${product}.port is not a number`,
      );
    }
    if (port > 65535) {
      throw new FirebaseError(
        CODES.invalidArgument,
        `emulator.${product}.port is out of range`,
      );
    }
    // An explicit null host counts as absent, like every string field.
    const host = endpoint.host === null ? undefined : endpoint.host;
    if (host !== undefined && typeof host !== "string") {
      throw new FirebaseError(CODES.invalidArgument, `emulator.${product}.host is not a string`);
    }
    return { host, port };
  };
  const options: FirebaseOptions = {
    apiKey: string("apiKey") ?? "",
    appId: string("appId") ?? "",
    projectId: string("projectId") ?? "",
    databaseURL: string("databaseUrl"),
    authURL: string("authUrl"),
    storageBucket: string("storageBucket"),
    emulator: {
      auth: endpoint("auth"),
      database: endpoint("database"),
      storage: endpoint("storage"),
    },
  };
  const invalid = validateOptions(options);
  if (invalid) throw invalid;
  return options;
}

function shape(value: unknown, message: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new FirebaseError(CODES.invalidArgument, message);
  }
  return value as Record<string, unknown>;
}
