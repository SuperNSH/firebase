// The honest empty state of a route whose screen has not been built yet.
import { Text, View } from "nsh";

export function Placeholder(props: { title: string; detail: string }) {
  return (
    <View class="items-start gap-2 p-6">
      <View class="rounded-full bg-muted px-3 py-1">
        <Text class="text-xs font-medium text-muted-foreground">Landing soon</Text>
      </View>
      <Text class="text-lg font-semibold text-foreground">{props.title}</Text>
      <Text class="text-sm text-muted-foreground">{props.detail}</Text>
      <Text class="text-sm text-muted-foreground">
        This route is wired to the shell; the screen arrives with its library.
      </Text>
    </View>
  );
}
