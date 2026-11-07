// The installations exercise UI the tests drive: one button per flow,
// one result line, and the restore-on-init read behind a press. Errors
// are shown with their stable code, native code and message (a
// FirebaseError's message already carries them). The entry that mounts it
// is src/installations/app.tsx, whose nsh.json grants the host names and
// the installations REST host.
import { signal, Text, View } from "nsh";
import { demoOptions, getApp, initializeApp, toFirebaseError } from "../src/core";
import { getInstallations, Installations } from "../src/installations";

export function InstallationsExerciseApp() {
  const app = initializeApp(demoOptions());
  const installations = getInstallations(app);
  const line = signal("idle");
  const restored = signal("restored: never");

  const call = (name: string, run: () => Promise<string>) => {
    run()
      .then((value) => line.set(`${name}: ${value}`))
      .catch((error) => line.set(`${name}: ${toFirebaseError(error).message}`));
  };

  const due = () => line.set(`due: ${installations.tokenDue()}`);
  const expires = () => {
    const at = installations.tokenExpiresAtMs();
    line.set(at === null ? "expires: none" : `expires: ${at}`);
  };

  // Restore-on-init: a fresh Installations over the same store, what the
  // app's next launch does. The restore is async, so the line lands when
  // it did.
  const reinstall = () => {
    const second = new Installations(getApp(app.name));
    void second.restored().then(() => {
      restored.set(
        `restored: fid=${second.id() ?? "none"} refresh=${second.refreshToken() ?? "none"}`,
      );
    });
  };

  const button = (id: string, label: string, onPress: () => void) => (
    <View testID={id} role="button" onPress={onPress} class="bg-primary p-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  return (
    <View class="flex-1 gap-2 bg-background p-4">
      <Text testID="line">{line()}</Text>
      <Text testID="restored">{restored()}</Text>
      {button("get-id", "get id", () => call("id", () => installations.getId()))}
      {button("get-token", "get token", () => call("token", () => installations.getToken()))}
      {button("force", "force", () => call("refresh", () => installations.getToken(true)))}
      {button("due", "due?", due)}
      {button("expires", "expires", expires)}
      {button("reinstall", "reinstall", reinstall)}
    </View>
  );
}
