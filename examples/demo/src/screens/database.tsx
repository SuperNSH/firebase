// The Realtime Database screen: a virtualized live list from the
// listener's signal and a push form. The listener follows the screen's
// lifecycle (onValue stops in its cleanup).
import { List, Text, View, signal } from "nsh";
import { demoDatabase } from "../firebase";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";

interface ItemRow {
  id: string;
  title: string;
}

const STATUS_LABEL: Record<string, string> = {
  connecting: "Connecting…",
  live: "Live",
  reconnecting: "Reconnecting…",
  revoked: "Stopped",
};

// One row of the map the ref holds: id first, then the title the push
// wrote, so a pushed value without a title still names itself.
function toRows(value: unknown): ItemRow[] {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return [];
  return Object.entries(value as Record<string, unknown>)
    .map(([id, entry]) => ({
      id,
      title:
        entry !== null && typeof entry === "object" && typeof (entry as { title?: unknown }).title === "string"
          ? (entry as { title: string }).title
          : id,
    }))
    .reverse();
}

export function DatabaseScreen() {
  const db = demoDatabase();
  const ref = db.ref("demo/items");
  // The value and connection state as signals: the list and the status
  // line update by themselves.
  const live = ref.onValue();
  const draft = signal("");

  const rows = () => toRows(live.data());

  const add = () => {
    const title = draft().trim();
    if (title === "") return;
    void ref.push({ title });
    draft.set("");
  };

  const clear = () => {
    void ref.set(null);
  };

  return (
    <View class="flex-1 bg-background">
      <View class="gap-2 border-b border-border px-4 py-3">
        <View class="flex-row items-center justify-between">
          <Text class="text-sm text-muted-foreground">
            {`${rows().length} items · ${STATUS_LABEL[live.status()] ?? live.status()}`}
          </Text>
          <Button
            variant="ghost"
            accessibilityLabel="Clear the list"
            testID="db-clear"
            onPress={clear}
          >
            <Text class="text-sm font-medium text-muted-foreground">Clear</Text>
          </Button>
        </View>
        <View class="flex-row items-center gap-2">
          <Input
            value={draft()}
            onChange={({ value }) => draft.set(value)}
            onSubmit={add}
            placeholder="New item"
            testID="db-draft"
            class="flex-1"
          />
          <Button
            accessibilityLabel="Add item"
            disabled={draft().trim() === ""}
            testID="db-add"
            onPress={add}
          >
            <Text class="text-sm font-medium text-primary-foreground">Add</Text>
          </Button>
        </View>
      </View>
      <List each={rows()} keyExtractor={(row) => row.id} estimatedItemSize={52} class="flex-1">
        {(row) => (
          <View class="border-b border-border px-4 py-3">
            <Text class="text-base text-foreground">{row().title}</Text>
            <Text class="text-xs text-muted-foreground">{row().id}</Text>
          </View>
        )}
      </List>
    </View>
  );
}
