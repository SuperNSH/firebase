// The demo's Firebase clients, over core's emulator config (demoOptions:
// demo-nsh on 127.0.0.1, every value fake). One instance per product at
// the module level, so they survive hot updates like the nav stack.
import { demoOptions, initializeApp, type FirebaseApp } from "nsh-firebase/core";
import { Auth } from "nsh-firebase/auth";
import { Database } from "nsh-firebase/database";
import { FirebaseStorage } from "nsh-firebase/storage";
import { Messaging } from "nsh-firebase/messaging";

let app: FirebaseApp | null = null;

export function demoApp(): FirebaseApp {
  if (!app) app = initializeApp(demoOptions());
  return app;
}

let auth: Auth | null = null;

export function demoAuth(): Auth {
  if (!auth) auth = new Auth(demoApp());
  return auth;
}

let database: Database | null = null;

export function demoDatabase(): Database {
  if (!database) database = new Database(demoApp());
  return database;
}

let storage: FirebaseStorage | null = null;

export function demoStorage(): FirebaseStorage {
  if (!storage) storage = new FirebaseStorage(demoApp());
  return storage;
}

export function demoMessaging(): Messaging {
  return new Messaging(demoApp().tier);
}
