// The home index: one row per product route the shell wires.
import { Scroll, Text, View } from "nsh";
import { nav } from "../../nav";
import { Button } from "../../ui/button";
import { PRODUCTS } from "../products";

export function HomeScreen() {
  return (
    <Scroll class="flex-1 bg-background">
      <View class="gap-2 p-4 pb-[env(safe-area-inset-bottom)]">
        <Each of={PRODUCTS} key={(product) => product.route}>
          {(product) => (
            <Button
              variant="ghost"
              testID={`row-${product().route}`}
              accessibilityLabel={product().title}
              class="w-full justify-between"
              onPress={() => nav.push({ name: product().route })}
            >
              <View class="flex-1 items-start gap-0.5">
                <Text class="text-base font-medium text-foreground">{product().title}</Text>
                <Text class="text-xs text-muted-foreground">{product().detail}</Text>
              </View>
              <Text class="text-muted-foreground">›</Text>
            </Button>
          )}
        </Each>
      </View>
    </Scroll>
  );
}
