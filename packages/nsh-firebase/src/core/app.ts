// The app registry: initializeApp / getApp, mirroring
// crates/nsh-firebase-core/src/app.rs.

import { EventBus } from "./bus";
import { CODES, FirebaseError } from "./error";
import { createTier, FirebaseTier } from "./tier";
import { FirebaseOptions, validateOptions } from "./options";

export class FirebaseApp {
  readonly name: string;
  readonly options: FirebaseOptions;
  readonly events: EventBus;
  readonly tier: FirebaseTier;

  constructor(name: string, options: FirebaseOptions) {
    this.name = name;
    this.options = options;
    this.events = new EventBus();
    this.tier = createTier();
  }
}

const apps = new Map<string, FirebaseApp>();

// Initializing an existing name returns the same app: options are only
// read once.
export function initializeApp(options: FirebaseOptions, name = "[DEFAULT]"): FirebaseApp {
  const invalid = validateOptions(options);
  if (invalid) throw invalid;
  const existing = apps.get(name);
  if (existing) return existing;
  const app = new FirebaseApp(name, options);
  apps.set(name, app);
  return app;
}

export function getApp(name = "[DEFAULT]"): FirebaseApp {
  const app = apps.get(name);
  if (!app) {
    throw new FirebaseError(CODES.noApp, `no app named \`${name}\`; call initializeApp first`);
  }
  return app;
}
