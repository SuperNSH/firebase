// The Cloud Messaging screen: token and permission status, with the pure
// tier's refusal rendered honestly — no silent no-op, no fake token.
import { Scroll, Text, View, signal } from "nsh";
import { demoMessaging } from "../firebase";
import { NEEDS_NATIVE_TIER } from "nsh-firebase/messaging";
import { Button } from "../../ui/button";

export function MessagingScreen() {
  const messaging = demoMessaging();
  const permission = signal<string | null>(null);
  const tokenLine = signal<string | null>(null);

  const checkPermission = () => {
    void messaging
      .permission()
      .then((granted) => permission.set(granted))
      .catch((error: { code?: string; message?: string }) =>
        permission.set(error.message ?? "unknown error"),
      );
  };

  const requestToken = () => {
    void messaging
      .token()
      .then((token) => tokenLine.set(`Token: ${token}`))
      .catch((error: { code?: string; message?: string }) =>
        tokenLine.set(error.message ?? "unknown error"),
      );
  };

  return (
    <Scroll class="flex-1 bg-background">
      <View class="gap-4 p-4 pb-[env(safe-area-inset-bottom)]">
        <View class="gap-2 rounded-(--radius) border border-border bg-card p-4">
          <Text class="text-sm font-medium text-foreground">Permission</Text>
          <Text class="text-sm text-muted-foreground" testID="messaging-permission">
            {permission() ?? "Not checked."}
          </Text>
          <Button
            variant="outline"
            accessibilityLabel="Check permission"
            testID="messaging-check"
            onPress={checkPermission}
          >
            <Text class="text-sm font-medium text-foreground">Check permission</Text>
          </Button>
        </View>
        <View class="gap-2 rounded-(--radius) border border-border bg-card p-4">
          <Text class="text-sm font-medium text-foreground">Registration token</Text>
          <Text class="text-sm text-muted-foreground" testID="messaging-token">
            {tokenLine() ?? "Not requested."}
          </Text>
          <Button
            variant="outline"
            accessibilityLabel="Request token"
            testID="messaging-request"
            onPress={requestToken}
          >
            <Text class="text-sm font-medium text-foreground">Request token</Text>
          </Button>
        </View>
        <Text class="text-xs text-muted-foreground">{NEEDS_NATIVE_TIER}</Text>
        <Text class="text-xs text-muted-foreground">
          This Mac is the pure tier: the requests above fail on purpose, and the screen says
          so instead of pretending. On the iOS and Android hosts the same calls reach the
          FCM SDK.
        </Text>
      </View>
    </Scroll>
  );
}
