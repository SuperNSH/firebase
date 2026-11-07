// The page's header on the desktops and the web; iOS and Android show the system's, from the page's title.
import { Text, View } from "nsh";
import { nav } from "../nav";
import { Button } from "../ui/button";

export function Header(props: { title: string }) {
  return (
    <View class="flex-row items-center gap-3 border-b border-border px-4 py-2">
      <If when={nav.routes().length > 1}>
        <Button variant="ghost" accessibilityLabel="Back" onPress={() => nav.pop()}>
          <Text class="text-sm font-medium text-foreground">Back</Text>
        </Button>
      </If>
      <Text class="text-sm font-semibold text-foreground">{props.title}</Text>
    </View>
  );
}
