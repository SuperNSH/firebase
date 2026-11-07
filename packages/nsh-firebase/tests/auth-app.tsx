// The auth exercise UI the tests drive: one button per flow, one result
// line, the currentUser signal as a text, and the restore-on-init read.
// Mirrors the Rust test component in
// crates/nsh-firebase-auth/tests/auth_ui.rs. The store is the app's
// Storage, so a test seeds it and reads it back through app.storage().
import { mount, signal, Text, View } from "nsh";
import type { Signal } from "nsh";
import { demoOptions, getApp, initializeApp } from "../src/core";
import { Auth } from "../src/auth";
import type { AuthUser, Credential } from "../src/auth";

const EMAIL = "demo@demo-nsh.test";
const PASSWORD = "fake-password-1";

function userLine(user: AuthUser | null): string {
  if (user === null) return "user: none";
  return (
    `user: ${user.uid} email=${user.email ?? "none"} anon=${user.isAnonymous}` +
    ` token=${user.idToken} refresh=${user.refreshToken}` +
    ` expires=${user.expiresAtMs} next=${user.nextRefreshAtMs}`
  );
}

function describe(result: Promise<AuthUser>, line: Signal<string>, name: string): void {
  result
    .then((user) => line.set(`${name}: ok ${user.uid}`))
    .catch((error) => {
      const err = error as { message?: string };
      line.set(`${name}: ${err.message ?? String(error)}`);
    });
}

export function AuthExerciseApp() {
  const app = initializeApp(demoOptions());
  const auth = new Auth(getApp(app.name));
  const line = signal("idle");
  const restored = signal("restored: never");

  const run = (name: string, flow: (auth: Auth) => Promise<AuthUser>) => {
    describe(flow(auth), line, name);
  };

  const signInEmpty = () => run("empty", (a) => a.signInWithEmailAndPassword("", ""));
  const signIn = () => run("signin", (a) => a.signInWithEmailAndPassword(EMAIL, PASSWORD));
  // The exists case reads as "exists", what the Rust component prints:
  // the sandbox run falls through to sign-in on an account the suite
  // already has.
  const signUp = () => {
    auth
      .createUserWithEmailAndPassword(EMAIL, PASSWORD)
      .then((user) => line.set(`signup: ok ${user.uid}`))
      .catch((error) => {
        const err = error as { code?: string; message?: string };
        line.set(
          err.code === "auth/email-already-in-use"
            ? "signup: exists"
            : `signup: ${err.message ?? String(error)}`,
        );
      });
  };
  const anon = () => run("anon", (a) => a.signInAnonymously());
  const custom = () => run("custom", (a) => a.signInWithCustomToken("custom-token-abc"));
  const credential = () =>
    run("credential", (a) =>
      a.signInWithCredential({ kind: "id-token", idToken: "foreign-id-token" } satisfies Credential),
    );

  const due = () => line.set(`due: ${auth.refreshDue()}`);

  const refreshNow = () => describe(auth.refreshNow(), line, "refresh");

  const signOut = () => {
    void auth.signOut().then(() => line.set("signed out"));
  };

  // Restore-on-init: a fresh Auth over the same store, what the app's
  // next launch does. The restore is async, so the line lands when it did.
  const reauth = () => {
    const second = new Auth(getApp(app.name));
    void second.authStateReady().then(() => {
      restored.set(`restored: ${userLine(second.currentUser())}`);
    });
  };

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="user">{userLine(auth.currentUser())}</Text>
      <Text testID="line">{line()}</Text>
      <Text testID="restored">{restored()}</Text>
      <View testID="signup" role="button" onPress={signUp} class="bg-primary p-2">
        <Text class="text-primary-foreground">sign up</Text>
      </View>
      <View testID="signin" role="button" onPress={signIn} class="bg-primary p-2">
        <Text class="text-primary-foreground">sign in</Text>
      </View>
      <View testID="empty" role="button" onPress={signInEmpty} class="bg-primary p-2">
        <Text class="text-primary-foreground">empty</Text>
      </View>
      <View testID="anon" role="button" onPress={anon} class="bg-primary p-2">
        <Text class="text-primary-foreground">anonymous</Text>
      </View>
      <View testID="custom" role="button" onPress={custom} class="bg-primary p-2">
        <Text class="text-primary-foreground">custom token</Text>
      </View>
      <View testID="credential" role="button" onPress={credential} class="bg-primary p-2">
        <Text class="text-primary-foreground">credential</Text>
      </View>
      <View testID="due" role="button" onPress={due} class="bg-primary p-2">
        <Text class="text-primary-foreground">due?</Text>
      </View>
      <View testID="refresh-now" role="button" onPress={refreshNow} class="bg-primary p-2">
        <Text class="text-primary-foreground">refresh</Text>
      </View>
      <View testID="signout" role="button" onPress={signOut} class="bg-primary p-2">
        <Text class="text-primary-foreground">sign out</Text>
      </View>
      <View testID="reauth" role="button" onPress={reauth} class="bg-primary p-2">
        <Text class="text-primary-foreground">reauth</Text>
      </View>
    </View>
  );
}

mount(AuthExerciseApp);
