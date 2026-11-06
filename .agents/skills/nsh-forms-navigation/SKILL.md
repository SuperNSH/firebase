---
name: nsh-forms-navigation
description: Build Native Shell (NSH) screens and forms that behave natively on every platform, covering navigation stacks, headers, unsaved-changes guards, the Android back order, dialogs, controlled inputs, field-to-field keyboard flow, autofill, and keeping inputs above the keyboard; use when adding routes, back handling, dialogs, sign-in or settings forms, or a chat composer to an NSH app.
---

# NSH forms and navigation

An NSH navigation stack is a real `UINavigationController` on iOS and a system toolbar with predictive back on Android, and an input is a real `UITextField` or `EditText`. That gives you swipe back, autofill and input methods for free, and it also adds rules. Questions the system asks at once, such as "may this page go back?", are answered from props you set beforehand, never from a callback. This skill covers those rules. Samples were compiled and tested on `nsh` 0.4.0 (e626a8d) on 2026-11-06.

## When to Use

- Adding pages, headers or routes to an NSH app.
- Asking "discard your changes?" before leaving a page.
- Handling Android's back button or the desktops' back keys.
- Building a dialog or sheet, or showing a system alert.
- Building a sign-in, settings, or checkout form.
- Keeping an input or a chat composer above the software keyboard.

## Set up the stack

Routes are data. In TSX, `createNavStack` holds them; in Rust, they are your own signal, and the stack's children are its pages, kept by key.

```tsx
// App.tsx
import { mount, Scroll, signal, Text, View } from "nsh";
import { AlertDialog, createNavStack, Input, NavPage, NavStack } from "nsh/base";

type Route = { name: "inbox" } | { name: "message"; id: number } | { name: "edit" };

const nav = createNavStack<Route>([{ name: "inbox" }]);

/** The desktops and the web draw this; iOS and Android show the system header from `title`. */
function Header(props: { title: string }) {
  return (
    <View class="h-12 flex-row items-center gap-3 border-b border-border px-4">
      <If when={nav.routes().length > 1}>
        <Text role="button" onPress={() => nav.pop()} class="text-primary active:opacity-60">
          Back
        </Text>
      </If>
      <Text class="text-base font-semibold text-foreground">{props.title}</Text>
    </View>
  );
}

function Inbox() {
  return (
    <NavPage title="Inbox" header={<Header title="Inbox" />}>
      <Scroll class="flex-1">
        <Each of={[1, 2, 3]}>
          {(id) => (
            <View role="button" onPress={() => nav.push({ name: "message", id: id() })} class="p-4 active:bg-accent">
              <Text class="text-foreground">{`Message ${id()}`}</Text>
            </View>
          )}
        </Each>
        <View role="button" onPress={() => nav.push({ name: "edit" })} class="p-4 active:bg-accent">
          <Text class="text-foreground">New note</Text>
        </View>
      </Scroll>
    </NavPage>
  );
}

function Message(props: { id: number }) {
  return (
    <NavPage title={`Message ${props.id}`} header={<Header title={`Message ${props.id}`} />}>
      <View class="flex-1 p-4">
        <Text class="text-foreground">{`The body of message ${props.id}.`}</Text>
      </View>
    </NavPage>
  );
}

/** Stays on the page while there is something to lose, on every way back. */
function EditNote() {
  const text = signal("");
  const dirty = () => text().length > 0;
  const confirm = async () => {
    const { action } = await AlertDialog.show({
      title: "Discard your changes?",
      actions: [
        { label: "Keep editing", role: "cancel", id: "keep" },
        { label: "Discard", role: "destructive", id: "discard" },
      ],
    });
    if (action === "discard") nav.pop();
  };
  return (
    <NavPage
      title="Edit"
      header={<Header title="Edit" />}
      popOnBack={!dirty()}
      onBackPress={() => {
        if (dirty()) confirm();
      }}
    >
      <Input
        value={text()}
        onChange={({ value }) => text.set(value)}
        placeholder="A note"
        class="m-4 text-base text-foreground"
      />
    </NavPage>
  );
}

function App() {
  return (
    <NavStack state={nav} class="flex-1 bg-background">
      {(route) => {
        switch (route.name) {
          case "inbox":
            return <Inbox />;
          case "message":
            return <Message id={route.id} />;
          case "edit":
            return <EditNote />;
        }
      }}
    </NavStack>
  );
}

mount(App);
```

```rust
// src/nav.rs
use nsh_base::{alert_dialog, input, nav_page, nav_stack};
use nsh_core::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Route {
    Inbox,
    Edit,
}

fn inbox(routes: Signal<Vec<Route>>) -> Screen {
    nav_page().title("Inbox").child(
        view()
            .on_press(move || routes.update(|r| r.push(Route::Edit)))
            .p_4()
            .child(text("New note")),
    )
}

fn edit(routes: Signal<Vec<Route>>) -> Screen {
    let draft = signal(String::new());
    let confirm = move || {
        spawn(async move {
            let choice = alert_dialog()
                .title("Discard your changes?")
                .cancel("keep", "Keep editing")
                .destructive("discard", "Discard")
                .show()
                .await;
            if choice.is_ok_and(|r| r.action.as_deref() == Some("discard")) {
                routes.update(|r| {
                    r.pop();
                });
            }
        });
    };
    nav_page()
        .title("Edit")
        .pop_on_back(move || draft.with(String::is_empty))
        .on_back_press(move || {
            if !draft.with(String::is_empty) {
                confirm();
            }
        })
        .child(
            input()
                .value(move || draft.get())
                .on_change(move |t: &str| draft.set(t.to_owned()))
                .placeholder("A note")
                .m(px(16.0)),
        )
}

#[component]
pub fn Nav() -> impl IntoElement {
    let routes = signal(vec![Route::Inbox]);
    nav_stack().flex_1().child(each(
        move || routes.get(),
        |route: &Route| format!("{route:?}"),
        move |route, index| {
            let page = match route.get_untracked() {
                Route::Inbox => inbox(routes),
                Route::Edit => edit(routes),
            };
            // The user went back natively: drop the route, and those above it.
            page.on_dismiss(move || routes.update(|r| r.truncate(index.get_untracked())))
        },
    ))
}
```

Rules that matter, from the [Navigation guide](https://nsh.dev/docs/navigation.md):

- **Two headers.** iOS and Android draw the system header from `title`, `headerRight` and friends. The desktops and the web draw the `header` slot, which is your own view. Set both, or the desktop build has no way back.
- **Pages stay mounted** while their route is in the stack, so going back shows the page as it was left. A page's `cleanup` runs when its route leaves.
- **TSX removes the route for you** when the user leaves natively, so `nav.routes()` always matches the screen. **Rust does not**: a page reports with `on_dismiss`, and you must drop the route, as above. Forgetting this leaves a stale route behind.
- **Keys identify Rust pages.** Two routes with the same key are the same page.
- **Create the TSX stack once,** at module level or in the root component, and import it where needed.
- **The render function runs once per route,** when pushed. The route is a plain value, and `index()` is reactive.

`nav` members: `routes()`, `push`, `pop(count?)`, `popToRoot()`, `replace`, `setRoutes`, `remove`. After sign-in, `setRoutes` keeps pages whose route value stays the same.

## Why popOnBack is a prop

When the user starts a swipe, the system asks at once whether the page may go, and the main thread never waits for app code. So the answer must already be in place. Keep `popOnBack` current as state changes, and treat `onBackPress` as a notice that the user tried, never as a veto. `pop_on_back` and `popOnBack` stop every way back: the back buttons, Android's back, the desktops' keys, the browser's back button and the swipe. Popping from code still works.

`popOnSwipe={false}` is lighter: it turns off only the interactive gesture, and the back buttons still pop.

## The Android back order

On Android, a back press is handled in this order:

1. The top dialog reports `onCancel`, and closes while `closeOnEscape` is true.
2. Your listeners (`BackHandler` from `nsh/mobile`, `on_back_press` in Rust), most recent first. Returning `true` keeps the stack from popping.
3. The top page reports the back press, and pops while `popOnBack` is true.
4. Nothing handled it, and the app exits.

A registered listener turns off the predictive back preview, because the app answers only once the gesture ends. So register a listener only while it may take the press:

```tsx
// back.tsx
import { effect, signal, Text, View } from "nsh";
import { BackHandler } from "nsh/mobile";

export function Editor() {
  const selecting = signal(false);
  effect(() => {
    if (!selecting()) return;
    // `on` returns the function that stops listening, and an effect runs what it returns.
    return BackHandler.on("press", () => {
      selecting.set(false);
      return true;
    });
  });
  return (
    <View onLongPress={() => selecting.set(true)} class="flex-1 p-4">
      <Text class="text-foreground">{selecting() ? "Selecting" : "Hold to select"}</Text>
    </View>
  );
}
```

## Dialogs

Use the semantic components rather than a view with an overlay. They bring focus, dismissal and the accessibility contract.

| Need | Use |
|---|---|
| A system alert or action sheet | `AlertDialog.show({ .. })`, `alert_dialog()` |
| Content over everything, your look | `Dialog`, `dialog()` |
| A panel from an edge | `Sheet` |
| A floating panel by its trigger | `Popover` |
| An actions list | `Menu` |

```tsx
// dialog.tsx
import { signal, Text, View } from "nsh";
import { Dialog } from "nsh/base";

export function DeleteFile(props: { onDelete: () => void }) {
  const open = signal(false);
  return (
    <View class="flex-1">
      <View role="button" onPress={() => open.set(true)} class="p-4">
        <Text class="text-destructive">Delete the file</Text>
      </View>
      <Dialog
        open={open()}
        animation="fade"
        onOpenChange={(e) => open.set(e.open)}
        backdropClass="bg-black/40"
        class="m-auto w-80 gap-3 rounded-2xl bg-card p-5"
      >
        <Text class="text-xl font-bold text-card-foreground">Delete the file?</Text>
        <View
          role="button"
          onPress={() => {
            open.set(false);
            props.onDelete();
          }}
        >
          <Text class="text-destructive">Delete</Text>
        </View>
      </Dialog>
    </View>
  );
}
```

Escape, Android's back, a backdrop press and a swipe down on a sheet close it natively. Control them with `closeOnEscape`, `closeOnBackdropPress` and `closeOnSwipe`, all true by default. Keep the open state in the app, so the trigger can show its open appearance. Inside a dialog, the safe-area insets are the dialog's own.

## Forms

Use controlled inputs for what the app reacts to as the user types. Move between fields with refs, and keep the keyboard up while focus moves by submitting without blurring.

```tsx
// signin.tsx
import { Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import type { InputRef } from "nsh";

export function SignIn(props: { onSubmit: (email: string, password: string) => Promise<string | null> }) {
  const email = signal("");
  const password = signal("");
  const error = signal<string | null>(null);
  const busy = signal(false);
  let passwordInput!: InputRef;

  const canSubmit = () => email().includes("@") && password().length > 0 && !busy();
  const submit = async () => {
    if (!canSubmit()) return;
    busy.set(true);
    error.set(await props.onSubmit(email(), password()));
    busy.set(false);
  };

  return (
    <Scroll class="flex-1" keyboardDismissMode="on-drag" keyboardShouldPersistTaps="handled">
      <View class="gap-3 p-4">
        <Input
          value={email()}
          onChange={({ value }) => email.set(value)}
          keyboardType="email-address"
          autoCapitalize="none"
          autoCorrect={false}
          autoComplete="email"
          returnKeyType="next"
          submitBehavior="submit"
          onSubmit={() => passwordInput.focus()}
          placeholder="E-mail"
          class="min-h-9 rounded-(--radius) border border-input px-3 py-2 text-sm focus:border-ring"
        />
        <Input
          ref={passwordInput}
          value={password()}
          onChange={({ value }) => password.set(value)}
          masked
          autoComplete="password"
          returnKeyType="go"
          onSubmit={submit}
          placeholder="Password"
          class="min-h-9 rounded-(--radius) border border-input px-3 py-2 text-sm focus:border-ring"
        />
        <If when={error()}>{(message) => <Text class="text-sm text-destructive">{message()}</Text>}</If>
        <View
          role="button"
          accessibilityLabel="Sign in"
          disabled={!canSubmit()}
          onPress={submit}
          class="min-h-9 items-center justify-center rounded-(--radius) bg-primary px-4 py-2 active:bg-primary/90 disabled:opacity-50"
        >
          <Text class="text-sm text-primary-foreground">{busy() ? "Signing in…" : "Sign in"}</Text>
        </View>
      </View>
    </Scroll>
  );
}
```

`input` in Rust comes from `nsh_base`, not the prelude:

```rust
// src/signin.rs
use nsh_base::{input, theme};
use nsh_core::{element::AutoFill, prelude::*};

fn field(placeholder: &'static str, value: Signal<String>) -> Input {
    input()
        .value(move || value.get())
        .on_change(move |text: &str| value.set(text.to_owned()))
        .placeholder(placeholder)
        .min_h_9()
        .px_3()
        .py_2()
        .text_sm()
        .rounded(move || theme().radius)
        .border_1()
        .border_color(move || theme().input)
        .focus(|s| s.border_color(move || theme().ring))
}

#[component]
pub fn SignIn() -> impl IntoElement {
    let email = signal(String::new());
    let password = signal(String::new());
    let password_input = element_ref();
    let status = signal(String::new());
    let can_submit = computed(move || email.with(|e| e.contains('@')) && password.with(|p| !p.is_empty()));
    let submit = move || {
        if can_submit.get_untracked() {
            dismiss_keyboard();
            status.set(format!("Signing in {}", email.get_untracked()));
        }
    };
    scroll()
        .flex_1()
        .keyboard_dismiss_mode(KeyboardDismissMode::OnDrag)
        .keyboard_taps(KeyboardTaps::Handled)
        .child(
            view()
                .p_4()
                .gap_3()
                .child(
                    field("E-mail", email)
                        .keyboard_type(KeyboardType::Email)
                        .auto_capitalize(AutoCapitalize::None)
                        .auto_fill(AutoFill::Email)
                        .return_key_type(ReturnKeyType::Next)
                        .submit_behavior(SubmitBehavior::Submit)
                        .on_submit(move |_: &str| password_input.focus()),
                )
                .child(
                    field("Password", password)
                        .element_ref(&password_input)
                        .masked(true)
                        .return_key_type(ReturnKeyType::Go)
                        .on_submit(move |_: &str| submit()),
                )
                .child(view().on_press(submit).child(text("Sign in")))
                .child(text(move || status.get())),
        )
}
```

The generated `Cargo.toml` has no `log` crate. Add it before using `log::info!`, as the docs' Rust samples do.

Details that catch people:

- `keyboardShouldPersistTaps="handled"` is what lets the submit button work while the keyboard is up. Without it, the first tap only dismisses the keyboard.
- Pick the keyboard by meaning. `number-pad` has no sign or decimal: use it for codes and PINs. `decimal-pad` is for amounts, and `numbers-and-punctuation` is for negatives on iOS.
- `autoComplete` values include `email`, `password`, `new-password` and `one-time-code`. Use `new-password` on sign-up so managers offer to generate one.
- `maxLength` and `submitBehavior` are enforced natively, so they need no handler.
- A return that commits an input method's composition (Chinese, Japanese) does not submit. Test multi-language apps with `onCompositionChange` in mind.
- `disabled` works on a `View` with `onPress`, as above. Use `Button` from your component library in real code.

## Keep inputs above the keyboard

Inside a scroll view or a list, this is automatic and native. NSH adds a bottom inset as tall as the keyboard and scrolls the focused input into view. So a long form is a `Scroll` of inputs, with no keyboard code.

Outside a scroll view, iOS moves the whole screen up. That is wrong for a composer pinned to the bottom, which should ride on the keyboard while the content above shrinks. Use padding from the environment variable:

```tsx
// composer.tsx
import { keyboardFrame, safeAreaInsets, Scroll, signal, Text, View } from "nsh";
import { Input } from "nsh/base";

export function ChatScreen() {
  const insets = safeAreaInsets();
  const draft = signal("");
  const sent = signal<string[]>([]);
  const send = () => {
    if (draft().trim()) sent.set([...sent(), draft()]);
    draft.set("");
  };
  return (
    <View style={{ flex: 1, paddingBottom: "env(keyboard-inset-height)" }}>
      <Scroll class="flex-1" keyboardDismissMode="interactive">
        <Each of={sent()}>{(message) => <Text class="p-2 text-foreground">{message()}</Text>}</Each>
      </Scroll>
      <View
        class="flex-row p-2"
        style={{ paddingBottom: 8 + (keyboardFrame().height > 0 ? 0 : insets().bottom) }}
      >
        <Input
          value={draft()}
          onChange={({ value }) => draft.set(value)}
          multiline
          submitBehavior="submit"
          returnKeyType="send"
          onSubmit={send}
          placeholder="Message"
          class="max-h-30 flex-1 text-sm text-foreground"
        />
      </View>
    </View>
  );
}
```

Rust uses `.pb_keyboard()`, with `keyboard_height()` and `safe_area()`. Padding shrinks the content while the background reaches the screen's bottom. A bottom margin, `.mb_keyboard()`, ends the whole view at the keyboard's top. On an Android host whose window resizes for the keyboard, the window shrinks and the variable is 0, so the layout still works. A host that uses `adjustPan` moves the whole window instead, so leave `windowSoftInputMode` unset or use `adjustResize`.

A multiline input scrolls inside its own height. To grow it, feed `onContentSizeChange` back into its height with a limit, and add the vertical padding yourself, because the reported height is the text's.

## Test the flows

Headless tests play the platform's part of going back, so all of this runs with no device. `app.back()` goes back as the user does, and asks the page when it has a guard. Dialogs are answered through `onDialog`.

```tsx
// App.test.tsx
import { expect, open, test } from "nsh/test";

test("back pops a clean page", async () => {
  const app = await open("./App.tsx", { clock: {} });
  await app.press("text:Message 2");
  await app.waitFor("text:The body of message 2.");
  await app.back();
  await app.waitFor("text:Message 1");
});

test("back asks before discarding, and stays when told to", async () => {
  const app = await open("./App.tsx", { clock: {} });
  await app.press("text:New note");
  await app.type("text:A note", "Milk");

  const titles: string[] = [];
  app.onDialog((dialog) => {
    titles.push(dialog.title ?? "");
    return "Keep editing";
  });
  await app.back();
  expect(titles).toEqual(["Discard your changes?"]);
  expect(app.value("text:Milk")).toBe("Milk");
});

test("back discards when confirmed", async () => {
  const app = await open("./App.tsx", { clock: {} });
  await app.press("text:New note");
  await app.type("text:A note", "Milk");
  app.onDialog(() => "Discard");
  await app.back();
  await app.waitFor("text:New note");
  expect(app.shows("text:Milk")).toBe(false);
});
```

A selector finds only the top screen of a stack and the top modal, so a query never sees a page underneath. Keyboard avoidance is platform behavior that a headless run cannot show, so check a form on a device with the real keyboard up.

## Copy these rules into the project

```markdown
## NSH navigation and form rules
- Every page sets both the system header (title) and the desktop and web header slot.
- A "may this page go?" answer is a prop kept current (popOnBack), never a callback result.
- Rust pages drop their route in on_dismiss. TSX stacks do it themselves.
- Back listeners are registered only while they may take the press.
- Alerts, dialogs, sheets, popovers and menus use their nsh/base components.
- Forms live in a Scroll with keyboardShouldPersistTaps="handled" and keyboardDismissMode set.
- Inputs set keyboardType, autoComplete and returnKeyType by meaning. Sign-up uses new-password.
- A bottom-pinned composer pads by env(keyboard-inset-height).
- Tests cover back on clean and dirty pages. Keyboard behavior is checked on a device.
```

## Review checklist

- [ ] The desktop and web `header` slot exists on each page, with a way back.
- [ ] Rust routes are removed in `on_dismiss`.
- [ ] Unsaved work is guarded with `popOnBack`, and the guard is tested.
- [ ] No back listener stays registered while idle.
- [ ] Form submit buttons work while the keyboard is up.
- [ ] Each input has the right keyboard, autofill and return key.
- [ ] No fixed bottom bar relies on the whole page moving up.

## Grounded in

- NSH Navigation guide, stacks, headers, `popOnBack`, dialogs, Android back order: https://nsh.dev/docs/navigation.md
- NSH Forms and the keyboard guide, inputs, autofill, focus, keyboard avoidance: https://nsh.dev/docs/forms-and-keyboard.md
- NSH `nsh/base` module, `AlertDialog` and navigation: https://nsh.dev/modules/nsh-base.md
- NSH Android guide, activity setup for back and the keyboard: https://nsh.dev/docs/android.md#activity-setup
- NSH Keyboard element, `env(keyboard-inset-height)`: https://nsh.dev/elements/keyboard.md
- NSH Testing guide, `back`, `onDialog` and selectors: https://nsh.dev/docs/testing-and-trace.md
- Official `nsh` skill, Rust UI reference: https://github.com/longbridge/nsh/tree/main/skills/nsh
