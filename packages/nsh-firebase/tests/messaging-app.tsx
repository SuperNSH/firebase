// The messaging exercise UI the tests drive: every facade call on a
// button, the three host→app events behind a listen press, errors shown
// with their stable code, native code and message. The entry that mounts
// it is src/messaging/app.tsx, whose nsh.json grants the host names.
import { signal, Text, View } from "nsh";
import { demoOptions, initializeApp, toFirebaseError } from "../src/core";
import { getMessaging, type RemoteMessage } from "../src/messaging";

// FirebaseError's message already carries the stable code and the native
// code, so the tests match on exactly what the facade produced.
function describeError(error: unknown): string {
  return toFirebaseError(error).message;
}

export function MessagingExerciseApp() {
  const line = signal("ready");
  const listenLine = signal("listen: not yet");
  const pressLine = signal("press: none");
  const messageLine = signal("message: none");
  const tokenEventLine = signal("token event: none");

  // Each test opens a fresh VM: initialize before the facade's first call.
  initializeApp(demoOptions());
  const messaging = getMessaging();

  const call = (name: string, run: () => Promise<string>) => {
    run()
      .then((value) => line.set(`${name}: ${value}`))
      .catch((error) => line.set(`${name}: ${describeError(error)}`));
  };

  const listen = () => {
    try {
      messaging.on("press", (payload) => pressLine.set(`press: ${payload.id}`));
      messaging.on("message", (payload: RemoteMessage) =>
        messageLine.set(`message: ${payload.messageId ?? "?"} ${payload.notification?.title ?? ""}`),
      );
      messaging.on("token", (payload) => tokenEventLine.set(`token event: ${payload.token}`));
      listenLine.set("listen: ok");
    } catch (error) {
      listenLine.set(`listen: ${describeError(error)}`);
    }
  };

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="state">{line()}</Text>
      {button("token", "token", () => call("token", async () => await messaging.token()))}
      {button("permission", "permission", () =>
        call("permission", async () => await messaging.permission()),
      )}
      {button("request-permission", "request", () =>
        call("request", async () => await messaging.requestPermission()),
      )}
      {button("show", "show", () =>
        call("show", async () =>
          String(await messaging.show({ id: "chat:42", title: "Ada", body: "Are you coming?" })),
        ),
      )}
      {button("show-hidden", "show hidden", () =>
        call("show hidden", async () =>
          String(await messaging.show({ id: "hidden", title: "Ada", body: "Are you coming?" })),
        ),
      )}
      {button("show-bare", "show bare", () =>
        call("show bare", async () =>
          String(await messaging.show({ id: "bare", title: "Ada", body: "Are you coming?" })),
        ),
      )}
      {button("show-empty", "show empty", () =>
        call("show empty", async () =>
          String(await messaging.show({ id: "quiet", title: "Hi", body: "" })),
        ),
      )}
      {button("show-empty-title", "show empty title", () =>
        call("show empty title", async () =>
          String(await messaging.show({ id: "quiet", title: "", body: "Are you coming?" })),
        ),
      )}
      {button("close", "close", () => call("close", async () => {
        await messaging.close("chat:42");
        return "ok";
      }))}
      {button("badge", "badge", () => call("badge", async () => {
        await messaging.setBadge(3);
        return "ok";
      }))}
      {button("subscribe", "sub", () => call("sub", async () => {
        await messaging.subscribe("news");
        return "ok";
      }))}
      {button("unsubscribe", "unsub", () => call("unsub", async () => {
        await messaging.unsubscribe("news");
        return "ok";
      }))}
      {button("subscribe-empty", "sub empty", () => call("sub empty", async () => {
        await messaging.subscribe("");
        return "ok";
      }))}
      {button("listen", "listen", listen)}
      <Text testID="listen-state">{listenLine()}</Text>
      <Text testID="press">{pressLine()}</Text>
      <Text testID="message">{messageLine()}</Text>
      <Text testID="token-event">{tokenEventLine()}</Text>
    </View>
  );
}
