// The Analytics screen: buttons that log events into the batching
// queue, with the batch status (queued, sent, dropped) on screen. The
// transport is the library's RecordingTransport, the stand-in a demo
// hands to getAnalytics; the collector transport is what the sandbox
// run points at the real sink.
import { Scroll, Text, View, signal } from "nsh";
import { QueuedAnalytics, RecordingTransport } from "nsh-firebase/analytics";
import { demoApp } from "../firebase";
import { Button } from "../../ui/button";

// One queue and transport per VM, module-level, so hot updates keep
// them; the batch size of 3 makes a batch land every third event.
const transport = new RecordingTransport();
const analytics = new QueuedAnalytics(demoApp(), {
  transport,
  forcePure: true,
  flushIntervalMs: 60_000,
  maxBatchSize: 3,
  maxQueueSize: 100,
  retryBaseMs: 1_000,
  retryMaxMs: 30_000,
  maxRetries: 5,
});

export function AnalyticsScreen() {
  // The library's counters are plain getters, not signals: every handler
  // bumps this version, and the status binding that reads it re-reads
  // them. onSend bumps it too, when a flush settles.
  const version = signal(0);
  const touch = () => version.update((n) => n + 1);
  analytics.onSend = touch;

  const status = () => {
    void version();
    return {
      queued: analytics.pending,
      sent: transport.posts.length,
      dropped: analytics.dropped,
    };
  };

  return (
    <Scroll class="flex-1 bg-background">
      <View class="gap-4 p-4 pb-[env(safe-area-inset-bottom)]">
        <View class="gap-1 rounded-(--radius) border border-border bg-card p-4" testID="analytics-status">
          <Text class="text-sm text-foreground">{`Queued: ${status().queued}`}</Text>
          <Text class="text-sm text-muted-foreground">{`Events sent: ${status().sent}`}</Text>
          <Text class="text-sm text-muted-foreground">{`Dropped: ${status().dropped}`}</Text>
        </View>
        <View class="flex-row flex-wrap gap-2">
          <Button
            accessibilityLabel="Log event"
            testID="analytics-log"
            onPress={() => {
              analytics.logEvent("demo_tap", { screen: "analytics" });
              touch();
            }}
          >
            <Text class="text-sm font-medium text-primary-foreground">Log event</Text>
          </Button>
          <Button
            variant="outline"
            accessibilityLabel="Log screen view"
            testID="analytics-screen"
            onPress={() => {
              analytics.logScreenView("Analytics");
              touch();
            }}
          >
            <Text class="text-sm font-medium text-foreground">Log screen view</Text>
          </Button>
          <Button
            variant="outline"
            accessibilityLabel="Flush now"
            testID="analytics-flush"
            onPress={() => {
              void analytics.flush();
              touch();
            }}
          >
            <Text class="text-sm font-medium text-foreground">Flush now</Text>
          </Button>
        </View>
        <Text class="text-xs text-muted-foreground">
          Events batch locally (a batch every 3 events or 60 s) and go to the demo's
          recording transport; the sandbox run points the same queue at the collector sink.
        </Text>
      </View>
    </Scroll>
  );
}
