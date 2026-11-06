---
name: nsh-component-library
description: Build and review the app-owned styled component layer of a Native Shell (NSH) app on top of nsh/base and nsh_base, covering the theme file, semantic tokens, part styling, state styles, variants, and how to prove the result actually draws; use when starting an NSH app's UI, wrapping a base component, adding a theme token, or debugging a control that passes its tests but shows nothing.
---

# NSH component library

Native Shell has no styled components. `nsh/base` (the `nsh_base` crate in Rust) gives you behavior: focus, keyboard, placement, dismissal, what screen readers hear. Everything visible is yours to build, once, in a UI module of your own. This skill is the how-to for that layer, in TSX and Rust. It adds to the official `nsh` and `nsh-design-guides` skills, which state the rule and the design standards but do not walk through building the layer. Samples here were compiled and run on `nsh` 0.4.0 (e626a8d) on 2026-11-06.

## When to Use

- Starting the UI of a new NSH app, before any feature screen exists.
- Wrapping a base component (`Button`, `Switch`, `Select`, `Dialog`, `Sheet`) in the app's own control.
- Adding a color, radius or font to the app, or restyling it for a brand.
- Debugging a control that passes `nsh test` and shows nothing on screen.
- Reviewing NSH UI code for raw colors, one-off styling and rebuilt controls.

Read the official `nsh-design-guides` skill first when you are choosing how something should look. This skill covers how to express that choice in code.

## The three layers

| Layer | TSX | Rust | Owns |
|---|---|---|---|
| Primitives | `nsh` | `nsh_core::prelude` | Views, text, signals, layout, typed styles |
| Base components | `nsh/base` | `nsh_base` | Behavior, state, focus, accessibility; no look |
| App UI module | your `ui/` folder | your `ui` module | Every color, size, variant and label |

Screens import from your UI module, never directly from the base layer. That keeps restyling to one place and stops per-screen drift. The [`nsh/base` page](https://nsh.dev/modules/nsh-base.md) describes the base layer as sitting between the primitives and the app's own library.

## Start with the theme file

Tokens come before components. Point `nsh.json` at a theme file, and list only what differs from the default theme, which is shadcn/ui's `neutral`.

```json
{
  "name": "Ledger",
  "id": "dev.example.ledger",
  "version": "1.0.0",
  "build": 1,
  "theme": "theme.json"
}
```

```json
{
  "cssVars": {
    "theme": { "radius": "0.5rem", "destructive-foreground": "#ffffff" },
    "light": { "primary": "#4f46e5", "primary-foreground": "#ffffff", "up": "#16a34a" },
    "dark": { "primary": "#818cf8", "primary-foreground": "#1e1b4b", "up": "#4ade80" }
  }
}
```

What that gives you, per the [Rust theme docs](https://nsh.dev/docs/rust.md#theme) and [theme tokens](https://nsh.dev/docs/tailwind.md#theme-tokens):

- `theme` sets both appearances, `light` and `dark` set one each. A value set only in `light` is used in both.
- The format is shadcn's `cssVars`, so themes exported from shadcn's theme page or tweakcn work unchanged.
- Any new name is a token. `"up"` above gives `bg-up`, `text-up/70`, `token("up")` in TSX and `theme().color("up")` in Rust.
- Numbers work the same way: `"radius-card": "14px"` gives `rounded-(--radius-card)` and `theme().number("radius-card")`.
- The default theme has `destructive` but no `destructive-foreground`, so add one if you draw text on that color, as above.
- `.nsh/types/nsh/tokens.d.ts` is regenerated from the theme, so `token("missing")` is a type error.
- A token follows the appearance on its own. Do not add `dark:` next to a token class.

Name tokens after roles (`up`, `surface-raised`), not components (`card-header-bg`). Code that copies a token's value into a signal stops following theme changes, so read the token where it is used.

Do not confuse the two theme keys. `"theme"` holds design tokens, which both classes and Rust read. `"tailwind"` extends Tailwind's scales for classes only, and a color named there wins over a token of the same name. Rust's typed methods ignore it. Prefer `"theme"` unless you need a custom scale.

## Wrap each base component once

TSX: variants are a small map of class strings, and props stay whole. The bundler warns when a component takes its props apart, because a destructured value is a snapshot and stops updating.

```tsx
// ui/button.tsx
import { type JSX, Text } from "nsh";
import { Button as BaseButton } from "nsh/base";

const variants = {
  primary: "bg-primary active:bg-primary/90",
  outline: "border border-border bg-background active:bg-accent",
  danger: "bg-destructive active:bg-destructive/90",
};

const labels = {
  primary: "text-primary-foreground",
  outline: "text-foreground",
  danger: "text-destructive-foreground",
};

export function Button(props: {
  label: string;
  variant?: keyof typeof variants;
  disabled?: boolean;
  onPress?: () => void;
  icon?: JSX.Element;
}) {
  const variant = () => props.variant ?? "primary";
  return (
    <BaseButton
      disabled={props.disabled}
      onPress={props.onPress}
      class={`min-h-9 flex-row items-center justify-center gap-2 rounded-(--radius) px-4 py-2 transition-colors disabled:opacity-50 focus:outline-2 focus:outline-ring ${variants[variant()]}`}
    >
      {props.icon}
      <Text class={`text-sm font-medium ${labels[variant()]}`}>{props.label}</Text>
    </BaseButton>
  );
}
```

Note `min-h-9` rather than `h-9`. Text follows the user's text size, so a fixed height clips it. The [Coding Guides](https://nsh.dev/docs/coding-guides.md) ask for minimum heights and padding on text containers.

A switch has parts, and each one must be styled or it does not exist visually:

```tsx
// ui/switch.tsx
import { Text } from "nsh";
import { Switch as BaseSwitch } from "nsh/base";

export function Switch(props: { label: string; checked: boolean; disabled?: boolean; onChange: (checked: boolean) => void }) {
  return (
    <BaseSwitch
      checked={props.checked}
      disabled={props.disabled}
      onChange={(e) => props.onChange(e.checked)}
      class="flex-row items-center gap-3 disabled:opacity-50"
      trackClass="w-11 rounded-full bg-muted p-0.5 transition-colors checked:bg-primary"
      thumbClass="size-5 rounded-full bg-background shadow-sm transition-transform checked:translate-x-5"
    >
      <Text class="text-sm text-foreground">{props.label}</Text>
    </BaseSwitch>
  );
}
```

Popups paint nothing behind their content, so give the popup its own background, border and shadow:

```tsx
// ui/select.tsx
import { Text, View } from "nsh";
import { Select as BaseSelect, type SelectItem } from "nsh/base";

export function Select(props: {
  label: string;
  items: SelectItem[];
  value: string | null;
  placeholder: string;
  onChange: (value: string | null) => void;
}) {
  const title = () => props.items.find((i) => i.value === props.value)?.title ?? props.placeholder;
  return (
    <BaseSelect
      items={props.items}
      value={props.value}
      onChange={(e) => props.onChange(e.value)}
      accessibilityLabel={props.label}
      offset={4}
      popupClass="rounded-lg border border-border bg-popover p-1 shadow-md"
      backdropClass="bg-black/40"
      itemClass="rounded-md px-2 py-1.5 text-sm text-popover-foreground focus:bg-accent selected:font-medium disabled:opacity-50"
      trigger={
        <View class="min-h-9 flex-row items-center justify-between rounded-(--radius) border border-input px-3 py-2 hover:bg-accent">
          <Text class="text-sm text-foreground">{title()}</Text>
          <Text class="text-muted-foreground">▾</Text>
        </View>
      }
    />
  );
}
```

`backdropClass` matters because a select is a popup on desktop and a bottom sheet on phones. Style for both.

A screen then composes only your controls, and owns the state:

```tsx
// App.tsx
import { mount, signal, Text, View } from "nsh";
import { Button } from "./ui/button";
import { Select } from "./ui/select";
import { Switch } from "./ui/switch";

const accounts = [
  { value: "main", title: "Main" },
  { value: "savings", title: "Savings" },
];

function App() {
  const alerts = signal(false);
  const account = signal<string | null>(null);
  const saved = signal(0);
  return (
    <View class="flex-1 gap-4 bg-background p-4 pt-[calc(env(safe-area-inset-top)+16px)]">
      <Text class="text-xl font-semibold text-foreground">Settings</Text>
      <Switch label="Price alerts" checked={alerts()} onChange={(next) => alerts.set(next)} />
      <Select
        label="Account"
        items={accounts}
        value={account()}
        placeholder="Pick an account"
        onChange={(next) => account.set(next)}
      />
      <Button label="Save" onPress={() => saved.update((n) => n + 1)} />
      <Button label="Delete account" variant="danger" disabled={account() === null} />
      <Text class="text-sm text-muted-foreground">Saved {saved()} times</Text>
    </View>
  );
}

mount(App);
```

## The same layer in Rust

Rust wraps the same way, with `theme()` and typed styles. A closure is a binding, so it follows the theme and the appearance natively.

```rust
// src/ui.rs
use nsh_base::{button, switch, theme};
use nsh_core::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    Primary,
    Outline,
}

/// The app's button: wrap once, use everywhere.
pub fn app_button(label: &'static str, variant: Variant, on_press: impl Fn() + 'static) -> impl IntoElement {
    button()
        .on_press(on_press)
        .min_h_9()
        .px_4()
        .py_2()
        .items_center()
        .justify_center()
        .rounded(move || theme().radius)
        .when(variant == Variant::Primary, |b| {
            b.bg(move || theme().primary).active(|s| s.opacity(0.85))
        })
        .when(variant == Variant::Outline, |b| {
            b.border_1()
                .border_color(move || theme().border)
                .active(|s| s.bg(move || theme().accent))
        })
        .focus(|s| s.outline(px(2.0)).outline_color(move || theme().ring))
        .disabled_style(|s| s.opacity(0.5))
        .child(text(label).text_sm().text_color(move || match variant {
            Variant::Primary => theme().primary_foreground,
            Variant::Outline => theme().foreground,
        }))
}

/// A labelled switch: the track and thumb draw nothing until styled here.
pub fn app_switch(label: &'static str, on: Signal<bool>) -> impl IntoElement {
    switch()
        .checked(on)
        .on_change(move |next: &bool| on.set(*next))
        .flex_row()
        .items_center()
        .gap_3()
        .track_style(|s| {
            s.w(px(44.0))
                .p(px(2.0))
                .rounded_full()
                .bg(move || theme().muted)
                .transition_colors()
                .checked(|s| s.bg(move || theme().primary))
        })
        .thumb_style(|s| {
            s.size(px(20.0))
                .rounded_full()
                .bg(move || theme().background)
                .shadow_sm()
                .transition_transform()
                .checked(|s| s.translate_x(px(20.0)))
        })
        .child(text(label).text_sm().text_color(move || theme().foreground))
}
```

Rust details that trip people up, from the official `nsh` skill's Rust UI reference:

- `.bg(..)` and `.px_4()` on a component style its root only. A part is reached through `*_style(|s| ..)` or `*_class("..")`.
- States on parts the app does not build are reachable only by class: `.item_class("focus:bg-accent selected:font-medium")`. This is the one case for a class string in Rust.
- Custom tokens are `theme().color("up")`. shadcn's tokens are fields and methods, such as `theme().primary` and `theme().muted_foreground`.

## Parts map

Generated on 0.4.0 from `.nsh/types/nsh/base.d.ts`. In TSX each part is `<part>Class` and `<part>Style`; in Rust the setters are snake case, such as `track_style`. Anything listed here is invisible until you style it.

| Component | Parts to style | Controlled by |
|---|---|---|
| `Switch` | `track`, `thumb` | `checked` |
| `Checkbox` | `indicator` | `checked` |
| `Progress` | `track`, `indicator` | `value` |
| `Slider` | `track`, `range`, `thumb`, `indicator` | `value` |
| `Tabs` | `indicator` | |
| `Select` | `popup`, `backdrop`, `list`, `item`, `group`, `search`, `clearButton` | `value`, `open` |
| `Combobox` | `popup`, `backdrop`, `input`, `list`, `item`, `group`, `footer`, `clearButton` | `value`, `open` |
| `Menu`, `ContextMenu` | `popup`, `backdrop`, `popupStarting`, `popupEnding` | `open` |
| `Popover`, `HoverCard` | `popup`, `popupStarting`, `popupEnding` | `open` |
| `Sheet` | `popup`, `backdrop` | `open` |
| `Dialog` | `backdrop` | `open` |
| `Tooltip`, `Toast` | `starting`, `ending` (and `swiping` on `Toast`) | `open` on `Tooltip` |
| `Command` | `search`, `searchBar`, `list`, `item`, `group`, `separator` | |
| `Tree` | `list`, `item` | |
| `NumberInput` | `input`, `controls`, `incrementButton`, `decrementButton` | `value` |
| `OtpInput` | `group`, `cell`, `caret` | `value` |
| `Calendar` | `header`, `body`, `month`, `row`, `item` | `value` |
| `DatePicker` | `popup`, `backdrop`, `content`, `header`, `body`, `month`, `row`, `item`, `calendar`, `time`, `timeField`, `segment`, `separator`, `presets`, `preset`, `clearButton` | `value`, `open` |
| `Avatar` | `image`, `fallback` | |
| `ResizablePanelGroup` | `handle`, `handleThumb` | |

For a component not listed, read its interface rather than guessing: `grep -n "interface SheetProps" .nsh/types/nsh/base.d.ts`.

The base layer gives every popup entry and exit states, `popupStarting` and `popupEnding`. A menu with a `transition-*` class and these two parts animates natively, with no signal involved.

## State styles, not signals

State styles apply natively the moment the state changes, with no round trip to your code. Reach for them before a signal.

| State | TSX class | Rust |
|---|---|---|
| Pressed | `active:` | `.active(\|s\| ..)` |
| Pointer over | `hover:` | `.hover(\|s\| ..)` |
| Focused | `focus:` | `.focus(\|s\| ..)` |
| Disabled | `disabled:` | `.disabled_style(\|s\| ..)` |
| Selected | `selected:` | `.selected_style(\|s\| ..)` |
| Checked | `checked:` | `.checked_style(\|s\| ..)` |
| Popup trigger open | `aria-expanded:` | `.expanded_style(\|s\| ..)` |
| Parent state, for a child | `group-active:`, `group-checked:` | `.group_active(\|s\| ..)` |

Later states win over earlier ones (hover, focus, active), and `disabled` wins over all. There is no built-in press feedback on any platform, so a button without an `active` style looks broken when pressed. An outward focus ring also needs room, because an ancestor with `overflow-hidden` clips it.

## Copy these rules into the project

```markdown
## NSH UI rules
- Screens import controls from the app's `ui` module, never from `nsh/base` or `nsh_base`.
- No raw hex, rgb, hsl or palette classes outside the theme file. A missing color is a new token first.
- No arbitrary sizes such as `p-[13px]` or `px(13.0)` without a comment naming the physical reason.
- Every part of a base component is styled, including its pressed, focused and disabled states.
- Text containers use min-height and padding, never a fixed height.
- Never rebuild Button, Menu, Popover, Sheet, Dialog or Select from plain views.
- Keep variants few and semantic. Do not add a variant for one call site.
- TSX components keep `props` whole; do not destructure them.
- `nsh build` warnings are findings: unknown classes and destructured props get fixed.
- A control is not done until a screenshot shows it in light and dark.
```

## Prove it draws

A headless test proves behavior and says nothing about the look. An unstyled `Switch` passes every test and shows nothing. So use three separate checks.

The headless test covers the behavior:

```tsx
// App.test.tsx
import { expect, open, test } from "nsh/test";

test("the danger button enables once an account is chosen", async () => {
  const app = await open("./App.tsx");
  expect(app.enabled("role:button[Delete account]")).toBe(false);
  expect(app.count("role:switch[Price alerts]")).toBe(1);
  await app.press("role:switch[Price alerts]");
  await app.press("text:Save");
  expect(app.texts()).toContain("Saved 1 times");
});
```

`nsh build` covers the static classes. It warns about any class that does not exist, in literals and in `classList` keys, but warnings do not fail the build, so read them. Classes built at run time are only checked when the runtime first sees them, which is another reason for a fixed variant map over string interpolation of color names.

A real target covers the pixels. A test can take a screenshot in both appearances on a real platform:

```tsx
// look.test.tsx
import { open, test } from "nsh/test";

test("settings in both appearances", async () => {
  const app = await open("./App.tsx", { colorScheme: "light" });
  await app.screenshot("settings-light.png");
  await app.setColorScheme("dark");
  await app.screenshot("settings-dark.png");
});
```

```bash
nsh test look.test.tsx --target macos
```

Headless has no drawing, so a screenshot step needs `--target` (`macos`, `linux`, `windows`, `ios`, `android`, `gpui` or `web`). Keep look tests in their own file so the plain `nsh test` loop stays fast. Then look at the PNGs: a test cannot tell you a thumb is invisible.

This check earns its place. An earlier draft of the button above reused `text-primary-foreground` on the danger variant. Every test passed, and the dark screenshot showed dark navy text on dim red, because this theme's dark `primary-foreground` is a dark color. Pair each background token with its own foreground token.

## Review checklist

- [ ] `nsh.json` names a theme file, and no presentation code holds a raw color.
- [ ] Every base component in use has one wrapper, and screens use the wrapper.
- [ ] Every part in the parts map is styled for each component in use.
- [ ] Pressed, focused and disabled states exist on every interactive control.
- [ ] Popups have a background, border and shadow, and a backdrop style for phones.
- [ ] Text containers grow with the user's text size.
- [ ] `nsh build` prints no `unknown class` and no destructured-props warning.
- [ ] A screenshot was looked at in light and dark, on a real target.

## Grounded in

- NSH `nsh/base` module, the three layers and component list: https://nsh.dev/modules/nsh-base.md
- NSH Tailwind guide, theme, theme tokens, variants and the class check: https://nsh.dev/docs/tailwind.md
- NSH Building with Rust, theme file format and token table: https://nsh.dev/docs/rust.md#theme
- NSH Coding Guides, behavior and presentation boundary, theme and styling rules: https://nsh.dev/docs/coding-guides.md
- NSH Switch element, parts and states: https://nsh.dev/elements/switch.md
- NSH Select element, presentation per device and parts: https://nsh.dev/elements/select.md
- NSH Testing, screenshots on a real platform: https://nsh.dev/docs/testing-and-trace.md#on-a-real-platform
- Official `nsh` skill, Rust UI reference and non-negotiables: https://github.com/longbridge/nsh/tree/main/skills/nsh
- Official `nsh-design-guides` skill: https://github.com/longbridge/nsh/tree/main/skills/nsh-design-guides
