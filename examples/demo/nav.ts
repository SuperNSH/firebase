// The navigation stack's state, created once; screens import it to navigate.
import { createNavStack } from "nsh/base";

export type Route =
  | { name: "home" }
  | { name: "auth" }
  | { name: "database" }
  | { name: "storage" }
  | { name: "messaging" }
  | { name: "analytics" }
  | { name: "monitor" };

export const nav = createNavStack<Route>([{ name: "home" }]);
