// The pure tier's transports over the app's fetch, with error mapping.
// Mirrors crates/nsh-firebase-core/src/transport.rs.

import { connect, type WebSocketConnection } from "websocket";
import { toFirebaseError } from "./error";

export interface FirebaseRequestInit {
  method?: string;
  headers?: Record<string, string>;
  body?: string;
}

export interface FirebaseResponse {
  status: number;
  ok: boolean;
  text: string;
}

// A refusal (a host not granted) or a failed request surfaces as a
// FirebaseError; an HTTP error status does not throw.
export async function requestText(url: string, init?: FirebaseRequestInit): Promise<FirebaseResponse> {
  try {
    const response = await fetch(url, init);
    return { status: response.status, ok: response.ok, text: await response.text() };
  } catch (error) {
    throw toFirebaseError(error);
  }
}

// The socket the database's live listeners use, refused like fetch.
export async function connectSocket(
  url: string,
  headers?: Record<string, string>,
): Promise<WebSocketConnection> {
  try {
    return await connect(url, headers ? { headers } : null);
  } catch (error) {
    throw toFirebaseError(error);
  }
}

// Joins a base URL and a path with exactly one slash.
export function joinUrl(base: string, path: string): string {
  return `${base.replace(/\/+$/, "")}/${path.replace(/^\/+/, "")}`;
}
