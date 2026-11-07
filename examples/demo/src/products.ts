// The demo's products: the routes the shell wires, in home's order. The single source of titles.
import type { Route } from "../nav";

export type ProductRoute = Exclude<Route["name"], "home">;

export interface ProductEntry {
  route: ProductRoute;
  title: string;
  detail: string;
}

export const PRODUCTS: ProductEntry[] = [
  { route: "auth", title: "Authentication", detail: "Email and anonymous sign-in, tokens, users" },
  { route: "database", title: "Realtime Database", detail: "Reads, writes and live listeners" },
  { route: "storage", title: "Cloud Storage", detail: "Uploads with progress, downloads, metadata" },
  { route: "messaging", title: "Cloud Messaging", detail: "Tokens, topics, received messages" },
  { route: "analytics", title: "Analytics", detail: "Events, screen views, user properties" },
  { route: "monitor", title: "Performance", detail: "Traces and per-call network timings" },
];

const BY_ROUTE = new Map(PRODUCTS.map((entry) => [entry.route, entry]));

/** The product a screen belongs to; a missing entry is a bug in the demo's own list. */
export function productEntry(route: ProductRoute): ProductEntry {
  const entry = BY_ROUTE.get(route);
  if (!entry) throw new Error(`no product entry for ${route}`);
  return entry;
}
