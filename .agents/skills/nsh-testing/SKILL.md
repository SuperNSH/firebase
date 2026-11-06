---
name: nsh-testing
description: Write fast, trustworthy tests for Native Shell (NSH) apps with nsh test and nsh_testing, covering selectors, network and storage stubs, manual clocks, playing the host app, real-platform runs, CI, and a clear account of what a headless pass does and does not prove; use when adding or fixing tests in an NSH app, when a test is flaky or slow, or when deciding whether a change needs a device.
---

# NSH testing

NSH runs your real app headless: the same reactive graph, renderer, components, layout and list virtualization as on a device, with display frames at 60 Hz, but no simulator and no window. That makes UI tests cheap enough to write first, and also makes it easy to over-trust them, because no pixel is ever drawn. This skill covers how to write these tests well and how to tell what a pass proves. Samples ran on `nsh` 0.4.0 (e626a8d) on 2026-11-06. For what makes a test good in general, use the `test-quality-guardrails`, `test-quality-tdd-agents` and `test-quality-audit` skills; this one covers NSH's tools.

## When to Use

- Adding tests to an NSH app, in TSX or Rust.
- A test is flaky, slow, or depends on real time or the real network.
- Deciding whether a headless pass is enough or a change needs a real platform.
- Setting up NSH tests in CI.
- Testing an app embedded in a host app, without the host.

## Pick the level

| Level | Command | Use it for | Cannot show |
|---|---|---|---|
| Type check | `nsh check` | TS types; `cargo check` on crates | Anything at run time |
| Bundle | `nsh build` | Unknown classes, text in a `View`, capability typos | Behavior |
| Headless TSX | `nsh test` | Behavior through the UI, for every mode | Pixels |
| Headless Rust | `cargo test` | Behavior through the UI, no QuickJS | Pixels |
| Real platform | `nsh test --target macos` | Real input, native dialogs, screenshots | Other platforms |
| Quick probe | `echo 'await app.press("Add")' \| nsh test App.tsx` | A throwaway step on the real network | Repeatability |
| Native tools | XCUITest, Espresso, Appium, Maestro | Flows with the OS, through `testID` | Speed |

In TSX, passing one of `nsh check` and `nsh build` proves little without the other. `nsh check` does not see what only the bundler knows, and `nsh build` does not type check.

## What a headless pass proves

Headless plays the platform's part of NSH's protocol, and nothing else. From the [Testing guide](https://nsh.dev/docs/testing-and-trace.md#what-headless-runs-play-of-the-platform):

| Headless does | Headless does not |
|---|---|
| Runs native animations frame by frame, to their end | Show how an animation looks |
| Pops screens and closes modals on `back` | Show a system header or a swipe |
| Answers dialogs through `onDialog` | Show the system's dialog |
| Enforces the app's `capabilities` | Prove the device's permissions |
| Records opened URLs | Open anything |
| Holds clipboard text | Read images or files from the clipboard |
| Answers picks through `onPick` | Show the picker |
| Lays out with an approximate text engine | Match device text sizes exactly |
| | Load images: no `onLoad` or `onError` comes |
| | Draw: an unstyled `Switch` passes and shows nothing |

So any task with a visible surface also needs a look on a real target, and the report should say so when that step was skipped.

## A first test, with its app

This app loads from the network, keeps a draft in storage and autosaves on a timer, so it needs all three of the tools below. `testID` gives steps a stable handle, while the role and label also serve screen readers.

```tsx
// App.tsx
import { mount, signal, Storage, Text, View } from "nsh";
import { Input } from "nsh/base";

type Todo = { id: number; title: string };

function App() {
  const todos = signal<Todo[]>([]);
  const failed = signal(false);
  const draft = signal("");
  const savedAt = signal<string | null>(null);

  fetch("https://api.example.com/todos")
    .then(async (response) => todos.set(await response.json()))
    .catch(() => failed.set(true));
  Storage.getItem("draft").then((value) => draft.set(value ?? ""));

  const add = async () => {
    const title = draft().trim();
    if (!title) return;
    const response = await fetch("https://api.example.com/todos", {
      method: "POST",
      body: JSON.stringify({ title }),
    });
    todos.set([...todos(), await response.json()]);
    draft.set("");
  };

  setInterval(async () => {
    await Storage.setItem("draft", draft());
    savedAt.set(new Date().toISOString().slice(11, 19));
  }, 10_000);

  return (
    <View class="flex-1 gap-3 bg-background p-4">
      <If when={failed()}>
        <Text role="alert" class="text-destructive">
          Could not load your todos
        </Text>
      </If>
      <Each of={todos()} key={(todo) => todo.id}>
        {(todo) => <Text class="text-foreground">{todo().title}</Text>}
      </Each>
      <Input
        testID="new-todo"
        accessibilityLabel="New todo"
        value={draft()}
        onChange={({ value }) => draft.set(value)}
        placeholder="What needs doing?"
      />
      <View role="button" accessibilityLabel="Add" onPress={add} class="bg-primary p-3">
        <Text class="text-primary-foreground">Add</Text>
      </View>
      <If when={savedAt()}>{(at) => <Text>{`Saved at ${at()}`}</Text>}</If>
    </View>
  );
}

mount(App);
```

```tsx
// App.test.tsx
import { beforeEach, describe, expect, open, test } from "nsh/test";
import type { App } from "nsh/test";

const API = "https://api.example.com/todos";

describe("todos", () => {
  let app: App;

  beforeEach(async () => {
    app = await open("./App.tsx", {
      clock: { time: new Date("2026-01-01T09:00:00Z") },
      storage: { draft: "Buy milk" },
      network: {
        [`GET ${API}`]: { body: [{ id: 1, title: "Walk the dog" }] },
        [`POST ${API}`]: (request) => ({
          status: 201,
          body: { id: 2, ...JSON.parse(request.body!) },
        }),
      },
    });
    await app.waitFor("text:Walk the dog");
  });

  test("restores the draft from storage", () => {
    expect(app.value("id:new-todo")).toBe("Buy milk");
  });

  test("adds a todo through the API", async () => {
    await app.press("role:button[Add]");
    await app.waitFor("text:Buy milk");
    expect(app.requests().map((r) => r.method)).toEqual(["GET", "POST"]);
    expect(JSON.parse(app.requests()[1].body!)).toEqual({ title: "Buy milk" });
    expect(app.value("id:new-todo")).toBe("");
  });

  test("autosaves ten seconds later, without waiting ten seconds", async () => {
    await app.clear("id:new-todo");
    await app.type("id:new-todo", "Call Sam");
    await app.clock!.runFor(10_000);
    await app.waitFor("text:Saved at 09:00:10");
    expect(app.storage().draft).toBe("Call Sam");
  });
});

test("says so when the list cannot load", async () => {
  const app = await open("./App.tsx", {
    network: { [`GET ${API}`]: { error: "offline" } },
  });
  await app.waitFor("role:alert");
  expect(app.texts()).toContain("Could not load your todos");
});
```

## Selectors

Write selectors the way a user finds things. A selector matches what the user sees, in the top modal, then on screen, and only the top page of a navigation stack.

| Selector | Finds | Prefer it when |
|---|---|---|
| `role:button[Save]` | A role and name | Almost always; it also proves the accessibility contract |
| `label:Close` | An accessibility label | Icon-only controls |
| `text:Saved` | A text, or an input by its text or placeholder | Checking what the user reads |
| `id:new-todo` | A `testID` | Inputs and repeated structure |
| `id:cart >> role:button[Remove]` | Inside a match | Repeated rows |
| `text:Delete >> nth=1` | One of several | Last resort; it breaks on reorder |
| `text:/^Buy/i` | A regex, in Rust's `regex` syntax, so no look-ahead or look-behind | Text with variable parts |

Rules that catch people:

- A step that acts on a view needs **exactly one** match. Several is a step error that says how to narrow it.
- A bare string tries a `testID`, then a label, then a text. Use a prefix when it matters.
- A text starting with `word:` takes a prefix: `text:tapped: solo`, or it is read as an unknown prefix.
- Match failures by `code`, not by message. The codes are stable: `ERR_NSH_TEST_NOT_FOUND`, `ERR_NSH_TEST_DISABLED`, `ETIMEDOUT`, `ERR_NSH_TEST_NO_BACK`, `ERR_NSH_TEST_UNSUPPORTED`.

```tsx
// fragment
await expect(app.press("Pay")).rejects.toMatchObject({ code: "ERR_NSH_TEST_DISABLED" });
```

## Waiting without flakes

A step resolves once the app rendered nothing for 100 ms and no finite native animation runs. So what the step changed is on screen when the next line runs. What comes later needs a wait:

| Situation | Use |
|---|---|
| A response, a timer, or a thread's answer will show something | `await app.waitFor("text:Saved")` |
| Something should go away | `await app.waitFor("text:Saving", { state: "gone" })` |
| No selector can say it | `await app.waitUntil(() => app.storage().note === "Milk")` |
| A timed behavior | A manual clock and `app.clock!.runFor(ms)` |

Never sleep with `app.wait(1000)` to get past a race. `wait` is for testing what happens as time passes.

A manual clock moves only when the test moves it, and the moves are immediate. Pass `clock: {}`, or `clock: { time }` to set the wall time. `Date`, `performance.now()` and `Intl`'s default date all follow it.

- `runFor(ms)` runs each timer, frame and delayed response as it comes due, in order.
- `fastForward(ms)` jumps, and the timers due meanwhile run once each, as a device waking from sleep does.
- `setSystemTime(time)` sets the wall time and moves no timer.
- `waitFor` moves the clock a frame at a time while it waits, by `timeout` at most.
- Anything that loops forever, such as a spinner, keeps running without blocking a step.

Use one by default for anything with an animation or a timer. A list test with a long `scroll` in the `nsh-lists-performance` skill took 13.7 s on the system clock and 18 ms on a manual one.

## The network and storage

A request no route matches fails, so tests never reach the network unasked. Routes are `"<METHOD> <URL>"`, a bare URL is a `GET`, and `*` matches any text. A response has `status`, `headers`, `body`, `error` and `delay`. A function route runs in the test file, so it can count or react.

Sockets and WebSockets open nothing unless `network` is `"live"`: `net.connect` and the `websocket` module's `connect` reject with `ERR_NSH_TEST_NETWORK`.

Tests run under the `capabilities` in `nsh.json`, as on a device, so forgetting to declare a host fails the test too, even when a route would answer it. We confirmed that on 0.4.0: with only `api.example.com` declared, a stubbed request to another host was refused with `ERR_ACCESS_DENIED`. That is useful, because it tests your declaration.

## Play the host app

An embedded app talks to its host through `host`, and in a test the test is the host. `host.params` is what the app reads at start, `host.handlers` answers calls and hears posts, and `app.host.send` sends events.

Annotate a handler's parameter. The docs' `async ({ id }) => ..` form fails `nsh check` on 0.4.0 with an implicit `any`, because the option's type is a union with `unknown`:

```tsx
// fragment
const app = await open("./App.tsx", {
  host: {
    params: { conversation: "c-42" },
    handlers: {
      "user.profile": async ({ id }: { id: number }) => ({ id, name: "Ada" }),
      "feature.flags": { search: true },
      analytics: null,
    },
  },
});
await app.press("Profile");
expect(app.host.calls()).toEqual([{ name: "user.profile", data: { id: 7 } }]);
await app.host.send("conversation.open", { id: "c-43" });
```

`capabilities.host` still applies, so the app's `nsh.json` must list what it calls. See the `nsh-host-embedding` skill for the host side, with a compiled version of this test.

## Tests in Rust

A Rust app's tests are integration tests of its crate, with the same steps and selectors. The docs' `Cargo.toml` samples show `nsh-engine = "0.1"`. Use your crate's own NSH version instead, because all NSH crates must come from one place:

```toml
[dev-dependencies]
nsh-engine = "0.4.0"
nsh-testing = "0.4.0"
```

```rust
// src/lib.rs
use nsh_base::button;
use nsh_core::prelude::*;

#[native_module("my-app/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

#[component]
pub fn App() -> impl IntoElement {
    let items = signal(Vec::<String>::new());
    let draft = signal(String::new());
    view()
        .flex_1()
        .p_4()
        .gap_3()
        .child(each(
            move || items.get(),
            |item: &String| item.clone(),
            |item, _| text(move || item.get()),
        ))
        .child(
            nsh_base::input()
                .test_id("new-todo")
                .value(move || draft.get())
                .on_change(move |t: &str| draft.set(t.to_owned())),
        )
        .child(
            button()
                .on_press(move || {
                    items.update(|i| i.push(draft.get_untracked()));
                    draft.set(String::new());
                })
                .child(text("Add")),
        )
}
```

```rust
// tests/app.rs
use nsh_engine::{EngineConfig, Root};
use nsh_testing::Headless;

#[test]
fn adds_a_todo() {
    let config = EngineConfig::new(Root::of::<nsh_app_my_app::App>());
    let mut app = Headless::start(config).unwrap();
    // `start` can return before the first frame, so wait for something first.
    app.wait_for("text:Add").unwrap();
    app.type_text("id:new-todo", "Buy milk").unwrap();
    app.press("text:Add").unwrap();
    app.wait_for("text:Buy milk").unwrap();
    assert_eq!(app.screen().count("text:Buy milk"), Ok(1));
}
```

Without that first `wait_for`, this test failed on 0.4.0 with `no input matches id:new-todo; on screen: []`, so do not copy the docs' sample that takes its first step straight after `start`. The crate an `nsh init my-app --template rust` makes is named `nsh-app-my-app`, so it imports as `nsh_app_my_app`. `EngineConfig` fields set what `open`'s options set: `storage` with `MemoryStorage::new(entries)`, `network` with `StubNetwork`, `viewport`, `environment`, and `clock` with `Clock::manual(time)`. `nsh test` in a directory with `"rust"` in `nsh.json` runs `cargo test` first, then the `*.test.tsx` files, so it is the one command for every mode.

## On a real platform

The same test file runs on a real development host, with the platform's own input. A press is a click at the view's middle, and `type` clicks the input and types its keys.

```bash
nsh test --target macos
```

```bash
nsh test tests/form.test.tsx --target ios
```

Targets are `macos`, `linux`, `windows`, `gpui`, `web`, `ios` and `android`. Other useful facts:

- `xvfb-run -a nsh test --target linux` runs on a machine with no display.
- `web` uses a headless Chrome, Edge or Chromium already installed, or `--browser <PATH>`.
- On macOS, windows open on a secondary display when there is one. `--display 1` picks the main one.
- `app.screenshot(path)` works only here. Headless has no drawing.
- `open` rejects options the target cannot honor with `ERR_NSH_TEST_UNSUPPORTED`. On iOS and Android that includes `width`, `height` and `host.handlers`, because the test does not answer calls there.

Keep those options out of tests you want to run everywhere, or split them into their own file. Run real targets for what headless cannot show, and leave the rest headless.

## CI

Headless needs no device or display, so it runs on Linux runners:

```yaml
# .github/workflows/test.yml
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # Install the nsh CLI here, the way you install it locally (nsh.dev/docs/installation).
      - run: nsh check
      - run: nsh build App.tsx
      - run: nsh test --junit test-results.xml
```

Any failure exits non-zero: a failed test, an app that does not build or renders nothing, an exception during a step. `--junit` writes a report per test for CI services. A plain `nsh build` warning does not fail CI, so if you want unknown classes to block, grep for `warn` and fail the step yourself. For performance guards, see the `nsh-lists-performance` skill.

## Break it to check it

A test that cannot fail is worse than no test. After writing a test, break the code on purpose and confirm the failure names the right thing. A failing NSH test prints the assertion, the line, and the texts on screen, which usually shows why it failed. The recycling test in `nsh-lists-performance` was checked this way, and it failed at the expected line, showing the heart on the wrong row.

## Copy these rules into the project

```markdown
## NSH testing rules
- The loop before done: nsh check, nsh build (read every warn), nsh test. In Rust: cargo check, cargo test.
- Selectors prefer role and label. nth= is a last resort.
- Tests use stubbed network routes. network: "live" needs a stated reason.
- Anything with a timer or an animation uses a manual clock (clock: {}).
- No app.wait() to get past a race. Use waitFor or waitUntil.
- Match failures by code, not by message.
- Lists are tested past the first screen and back.
- A headless pass is a behavior claim, not a visual one. Visible changes also get a look on a real target,
  and the report says when that step was skipped.
- New tests are seen to fail once, by breaking the code on purpose.
```

## Review checklist

- [ ] Each test opens its own app, and shares no state.
- [ ] Steps use role, label or id rather than position.
- [ ] No real network, and no real time, without a stated reason.
- [ ] Failure paths are tested: offline, refused, empty.
- [ ] `capabilities` in `nsh.json` are in force and tested.
- [ ] The test was seen to fail.
- [ ] Visible work was checked on a real target, or the report says it was not.

## Grounded in

- NSH Testing guide, headless runs, steps, selectors, clocks, real platforms and CI: https://nsh.dev/docs/testing-and-trace.md
- NSH CLI reference, `nsh test` flags: https://nsh.dev/docs/cli.md
- NSH Building with Rust, testing a Rust root: https://nsh.dev/docs/rust.md#testing
- NSH Sandbox and capabilities, tests run under the declaration: https://nsh.dev/docs/sandbox-and-capabilities.md
- Official `nsh` skill, verification loop and Rust testing reference: https://github.com/longbridge/nsh/tree/main/skills/nsh
