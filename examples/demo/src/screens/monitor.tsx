// The Performance route: which calls took what time, slowest on top.
// The calls are live against the collector on 127.0.0.1:18742 (its
// deliberately slow route, two fast routes and the sink), measured by
// the shared monitor the instrumented fetch feeds, so the list is the
// same ring a real app's calls would fill.
import { List, Text, View, cleanup, signal } from "nsh";
import {
  instrumentFetch,
  isNetworkError,
  monitorRows,
  sharedMonitor,
  type NetworkEvent,
} from "nsh-firebase/monitor";
import { productEntry } from "../products";

const COLLECTOR = "http://127.0.0.1:18742";
// The collector's deliberately slow route: the row every demo looks for.
const SLOW_ROUTE = `${COLLECTOR}/slow?ms=1200`;
const HEALTH_ROUTE = `${COLLECTOR}/health`;
const ASSERT_ROUTE = `${COLLECTOR}/assert?contains=demo&count=0`;
const COLLECT_ROUTE = `${COLLECTOR}/collect`;

// The visible filters. All is the unfiltered, slowest-first view; Slow
// keeps the calls at or above a second; Errors keeps the failures.
const FILTERS = [
  { key: "all", label: "All" },
  { key: "slow", label: "≥ 1 s" },
  { key: "errors", label: "Errors" },
] as const;

type FilterKey = (typeof FILTERS)[number]["key"];

function isSlow(event: NetworkEvent): boolean {
  return event.durationMs >= 1000;
}

// Milliseconds as the rows read them; the clock is the engine's, so the
// number is the measured call, not wall time.
function ms(value: number | null): string {
  return value === null ? "—" : `${Math.round(value)} ms`;
}

export function MonitorScreen() {
  const entry = productEntry("monitor");
  const rows = signal<NetworkEvent[]>([]);
  const pending = signal(0);
  const filter = signal<FilterKey>("all");

  const applyView = () => {
    const filterKey = filter();
    let view = monitorRows(sharedMonitor.events());
    if (filterKey === "slow") view = view.filter(isSlow);
    if (filterKey === "errors") view = view.filter(isNetworkError);
    rows.set(view);
  };

  // The list follows the ring: every recorded call re-sorts the view.
  const stop = sharedMonitor.subscribe(() => applyView());
  // The unsubscribe reads no signal, so it is safe after disposal.
  cleanup(() => stop());

  const load = () => {
    const fetchMeasured = instrumentFetch(sharedMonitor);
    const calls: Array<Promise<unknown>> = [
      fetchMeasured(SLOW_ROUTE),
      fetchMeasured(HEALTH_ROUTE),
      fetchMeasured(ASSERT_ROUTE),
      fetchMeasured(COLLECT_ROUTE, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ demo: "monitor-call" }),
      }),
    ];
    pending.set(calls.length);
    for (const call of calls) {
      // A failed call is already an error event in the ring; its
      // rejection has nowhere better to go.
      call.catch(() => {}).finally(() => {
        pending.update((n) => Math.max(0, n - 1));
      });
    }
  };

  const setFilter = (key: FilterKey) => {
    filter.set(key);
    applyView();
  };

  void load();

  // Both header numbers read the whole ring — a filter narrows the list
  // below, not the summary's scope. The rows() read is what tracks the
  // binding: the ring itself is plain state, so without it the summary
  // would show its mount-time snapshot forever.
  const summary = () => {
    void rows();
    return {
      calls: sharedMonitor.length,
      errors: sharedMonitor.events().filter(isNetworkError).length,
    };
  };
  const durationTone = (event: NetworkEvent) =>
    isNetworkError(event)
      ? "text-destructive"
      : isSlow(event)
        ? "font-semibold text-foreground"
        : "text-foreground";

  return (
    <View class="flex-1 bg-background">
      <View class="gap-2 border-b border-border px-4 py-3">
        <View class="flex-row items-center justify-between">
          <Text class="text-sm text-muted-foreground">
            {`${summary().calls} calls · ${summary().errors} errors`}
          </Text>
          <If when={pending() > 0}>
            <Text class="text-xs text-muted-foreground">Measuring…</Text>
          </If>
        </View>
        <View class="flex-row flex-wrap gap-2">
          <Each of={FILTERS} key="index">
            {(chip) => (
              <View
                testID={`filter-${chip().key}`}
                role="button"
                accessibilityLabel={`Filter ${chip().label}`}
                onPress={() => setFilter(chip().key)}
                class={`min-h-8 flex-row items-center rounded-full px-3 ${
                  filter() === chip().key ? "bg-primary" : "border border-border bg-background"
                }`}
              >
                <Text
                  class={`text-xs font-medium ${
                    filter() === chip().key ? "text-primary-foreground" : "text-foreground"
                  }`}
                >
                  {chip().label}
                </Text>
              </View>
            )}
          </Each>
          <View class="flex-1" />
          <View
            testID="reload"
            role="button"
            accessibilityLabel="Measure the calls again"
            onPress={load}
            class="min-h-8 flex-row items-center rounded-full bg-secondary px-3 active:bg-accent"
          >
            <Text class="text-xs font-medium text-secondary-foreground">Reload</Text>
          </View>
        </View>
        <Text class="text-xs text-muted-foreground">{entry.detail}</Text>
      </View>
      <If
        when={rows().length > 0}
        else={
          <View class="items-start gap-2 p-6">
            <Text class="text-sm text-muted-foreground">
              {pending() > 0
                ? "Measuring the collector's slow and fast routes…"
                : // A filtered empty is not an empty ring: the copy says
                  // which one it is, so Reload is only offered when the
                  // ring holds nothing to filter.
                  sharedMonitor.length > 0
                  ? "No calls match this filter."
                  : "No calls yet. Press Reload to measure them."}
            </Text>
          </View>
        }
      >
        <List
          each={rows()}
          keyExtractor={(event) => event.id}
          estimatedItemSize={64}
          class="flex-1"
        >
          {(event, index) => (
            <View testID={`call-${index()}`} class="border-b border-border px-4 py-3">
              <View class="flex-row items-center gap-2">
                <View class="min-h-6 flex-row items-center rounded bg-muted px-2">
                  <Text class="text-xs font-semibold text-foreground">{event().method}</Text>
                </View>
                <Text class="flex-1 text-sm text-foreground">{event().path}</Text>
                <Text class={`text-sm tabular-nums ${durationTone(event())}`}>
                  {ms(event().durationMs)}
                </Text>
              </View>
              <View class="mt-1 flex-row items-center gap-2">
                <Text class="text-xs text-muted-foreground">{event().host}</Text>
                <Text class="text-xs text-muted-foreground">{`ttfb ${ms(event().ttfbMs)}`}</Text>
                <Text
                  class={`text-xs ${isNetworkError(event()) ? "text-destructive" : "text-muted-foreground"}`}
                >
                  {event().status ?? "failed"}
                </Text>
                <View class="flex-1" />
                <Text class="text-xs text-muted-foreground">{event().initiator}</Text>
              </View>
            </View>
          )}
        </List>
      </If>
    </View>
  );
}
