// The crashlytics exercise UI the tests drive: every facade call on a
// button, errors shown with their stable code, native code and message.
// The entry that mounts it is src/crashlytics/app.tsx, whose nsh.json
// grants the host names. Mirrors tests/messaging-app.tsx; there are no
// host→app events — the native SDK reports crashes on its own.
import { signal, Text, View } from "nsh";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";
import { getCrashlytics, type CustomKeyValue } from "../src/crashlytics";

export function CrashlyticsExerciseApp() {
  const line = signal("ready");

  // Each test opens a fresh VM: initialize before the facade's first
  // call. The tier is the open's decision: host options play the native
  // tier, none plays the pure tier.
  initializeApp(demoOptions());
  const crashlytics = getCrashlytics();

  const call = (name: string, run: () => Promise<string>) => {
    run()
      .then((value) => line.set(`${name}: ${value}`))
      .catch((error) => line.set(`${name}: ${toFirebaseError(error).message}`));
  };

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      {button("log", "log", () => call("log", async () => {
        await crashlytics.log("payment screen shown");
        return "ok";
      }))}
      {button("user-id", "user id", () => call("user id", async () => {
        await crashlytics.setUserId("user-41");
        return "ok";
      }))}
      {button("key", "key", () => call("key", async () => {
        await crashlytics.setAttribute("difficulty", "hard");
        return "ok";
      }))}
      {button("keys", "keys", () => call("keys", async () => {
        await crashlytics.setAttributes({ level: 7, paid: true });
        return "ok";
      }))}
      {button("key-bad", "key bad", () =>
        call("key bad", async () => {
          // The value is not one of the SDK's custom-key types: refused
          // before the host, whatever tier this is.
          await crashlytics.setAttribute("bad", null as unknown as CustomKeyValue);
          return "ok";
        }),
      )}
      {button("record-error", "record error", () => call("record error", async () => {
        await crashlytics.recordError({ code: 401, message: "payment declined" });
        return "ok";
      }))}
      {button("record-error-stack", "record error stack", () =>
        call("record error stack", async () => {
          await crashlytics.recordError({
            code: 500,
            message: "boom",
            stack: "at run (app.tsx:1)",
          });
          return "ok";
        }),
      )}
      {button("send", "send", () => call("send", async () => {
        await crashlytics.sendUnsentReports();
        return "ok";
      }))}
      {button("did-crash", "did crash", () => call("did crash", async () =>
        String(await crashlytics.didCrashOnPreviousExecution()),
      ))}
    </View>
  );
}
