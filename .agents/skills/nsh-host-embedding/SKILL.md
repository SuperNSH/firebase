---
name: nsh-host-embedding
description: Embed Native Shell (NSH) apps inside an existing iOS, Android, macOS, GPUI or web host, covering the host view per platform, bundles, the configure-then-load order, the two-way host channel, native components, theme overrides, and limits; use when adding an NSH screen to a native app, wiring host.call and host.on, exposing a native view such as a map to NSH code, or debugging why an embedded app cannot reach its host.
---

# NSH host embedding

Embedding is what Native Shell is designed around: a native app shows one or more NSH apps inside its own UI, and each one is an independent engine with its own thread, VM, limits and capabilities. The official `nsh` skill builds apps and barely touches this side. This skill covers the host app's half and how the two halves meet. The TSX, Swift, Java and macOS Rust samples here were type-checked against `nsh` 0.4.0 (e626a8d) and its shipped frameworks on 2026-11-06, and the TSX tests ran. Nothing here ran on a device, so the first real run still belongs in your report. Kotlin, GPUI and web samples are marked as not compiled. To run code that arrives after release, also read `nsh-signed-delivery`.

## When to Use

- Adding an NSH screen to an existing iOS, Android, Mac, Windows, Linux, GPUI or web app.
- Passing launch parameters, calling host functions, or sending events either way.
- Exposing a host view such as a map, camera or chart to NSH code.
- Branding one NSH app differently across several host apps.
- An embedded app shows errors like `ERR_ACCESS_DENIED`, or a component renders as an empty view.

## Which view on which platform

| Host | View | Get it from | Runs |
|---|---|---|---|
| iOS, Swift | `NshAppView`, a `UIView` | `NativeShell.xcframework` | Bundles, signed packages |
| iOS, Rust | `AppView` | `nsh-platform-ios` crate | Rust root, bundles |
| Android | `NshView`, a `FrameLayout` | `nsh.aar`, API 28+ | Bundles, signed packages |
| macOS | `AppView`, an `NSView` | `nsh-platform-macos` crate | Rust root, bundles |
| Windows | `AppView` in a Win32 app | `nsh-platform-windows` crate | Rust root, bundles |
| Linux | `AppView` in a GTK app | `nsh-platform-linux` crate | Rust root, bundles |
| GPUI, GPUI Kit | `AppView`, an `Entity` | `nsh-platform-gpui`, from the NSH repository | Rust root, bundles |
| Web | `createPlayer` from `nsh.js`, or an iframe | `nsh package --target web` | TSX modules; a Rust app via iframe |

Each view is one engine, and there are no global singletons, so one app can show several at once, for example one per tab. On a Homebrew install the frameworks are at `$(brew --prefix nsh)/libexec/embed/ios/` and `.../embed/android/`. Each comes in two variants: the `Dev` one (`NativeShellDev.xcframework`, `nsh-dev.aar`) also connects to a dev server. Link it in Debug only.

## Build the bundle

```bash
nsh build App.tsx --release --out MyApp/nsh
```

| File | Contents |
|---|---|
| `bundle.nshc` | Bytecode, the fastest to load |
| `bundle.js`, `bundle.js.map` | The same app as source, and its map |
| `capabilities.json` | What the app declares, when `nsh.json` has any |
| `app.json` | Its id and metadata |
| `assets/` | Imported images and fonts |

On iOS, add the folder as a folder reference named `nsh`. On Android, build into the app's assets. Bytecode loads only from inside the app bundle or APK, or from a verified package, because QuickJS does not validate bytecode and crafted bytecode can corrupt memory. A file on storage must be JS source, or better, a signed package.

## The order of setup

Most settings apply from the next load. Set them first, or they silently do nothing for the app already running.

1. Register native components.
2. Set limits, granted capabilities, storage and cache directories.
3. Set launch parameters.
4. Add host handlers. A call with no handler fails at once.
5. Load the bundle or package.

Locale, font scale and theme overrides apply at once, and also to later loads.

## iOS

```swift
// EmbedController.swift
import NativeShell
import UIKit

final class Badge: NshComponent {
    let view = UIView()
    private let label = UILabel()
    private let context: NshComponentContext
    private var taps = 0

    init(_ context: NshComponentContext) {
        self.context = context
        label.textAlignment = .center
        label.frame = view.bounds
        label.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(label)
        view.addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped)))
    }

    func setProp(_ name: String, _ value: Any?) {
        guard name == "text" else { return }
        label.text = value as? String
        let size = label.intrinsicContentSize
        // Report the size, so NSH can lay out a component that sizes to its content.
        context.setSize(CGSize(width: size.width + 32, height: size.height + 20))
    }

    func command(_ name: String, _ args: Any?) {
        guard name == "pulse" else { return }
        view.alpha = 0.3
        UIView.animate(withDuration: 0.6) { self.view.alpha = 1 }
    }

    @objc private func tapped() {
        taps += 1
        context.emit("tap", ["count": taps])
    }
}

final class EmbedController: UIViewController {
    private let app = NshAppView()

    override func viewDidLoad() {
        super.viewDidLoad()
        app.translatesAutoresizingMaskIntoConstraints = false
        app.onError = { message in print("nsh error: \(message)") }
        view.addSubview(app)
        NSLayoutConstraint.activate([
            app.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            app.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            app.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            app.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])

        app.register(component: "embed.badge") { context in Badge(context) }
        app.limits = NshLimits(memoryMB: 128, callBudget: 1, maxNodes: 50_000)
        app.params = ["conversation": "c-42"]

        // An async handler: its result answers the call, and a throw rejects it.
        app.handle("user.profile") { data in
            let id = (data as? [String: Any])?["id"] as? Int ?? 0
            return ["id": id, "name": "Ada"]
        }
        // A post has no answer.
        app.handle("analytics") { data, _ in print("analytics", data ?? "") }

        do {
            try app.grantCapabilities(#"{ "network": { "hosts": ["api.example.com"] }, "host": ["user.*", "analytics"] }"#)
            guard let url = Bundle.main.url(forResource: "bundle", withExtension: "nshc", subdirectory: "nsh") else { return }
            try app.loadBundle(at: url)
        } catch {
            print("cannot load: \(error)")
        }
    }

    func conversationTapped(_ id: String) {
        // Kept until the app listens, so it is safe to send during startup.
        app.send("conversation.open", ["id": id])
    }

    // Let the embedded app's pages and dialogs decide the status bar.
    override var preferredStatusBarStyle: UIStatusBarStyle { NshAppView.preferredStatusBarStyle }
    override var prefersStatusBarHidden: Bool { NshAppView.prefersStatusBarHidden }
    override var preferredStatusBarUpdateAnimation: UIStatusBarAnimation {
        NshAppView.preferredStatusBarUpdateAnimation
    }
}
```

Xcode setup, from the [iOS guide](https://nsh.dev/docs/ios.md):

- Drag the framework into **Frameworks, Libraries, and Embedded Content**. The default static framework is **Do Not Embed**. A `--dynamic` build is **Embed & Sign**.
- Set `CADisableMinimumFrameDurationOnPhone` to `YES` in `Info.plist`, or animations stay below 120 Hz on ProMotion iPhones.
- Minimum deployment target is iOS 15.0. `NshAppView` is isolated to the main actor.
- Point a Debug build at a dev server with `try app.connectDevServer(URL(string: "ws://127.0.0.1:8790/ws")!)`, while `nsh dev App.tsx` runs. A device needs the LAN address `nsh dev` prints, plus local network access and App Transport Security settings that allow it.

## Android

```java
// dev/example/embed/MainActivity.java
package dev.example.embed;

import android.app.Activity;
import android.os.Bundle;
import android.util.Log;

import com.longbridge.nsh.NshView;

import java.util.Map;

public final class MainActivity extends Activity {
    private NshView nsh;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        nsh = new NshView(this);
        nsh.setErrorListener(message -> Log.e("nsh", message));
        setContentView(nsh);

        nsh.setLimits(128, 0, 0, 1000, 50_000); // memory MB, stack KB, load ms, call ms, nodes; 0 keeps a default
        nsh.setParams(Map.of("conversation", "c-42"));

        // Handlers run on the UI thread, and answer once, from any thread.
        nsh.handle("user.profile", (data, reply) -> {
            Object id = ((Map<?, ?>) data).get("id");
            reply.resolve(Map.of("id", id, "name", "Ada"));
        });
        nsh.handle("analytics", (data, reply) -> Log.i("analytics", String.valueOf(data)));

        // Returns false for invalid JSON, and is then ignored, so check it.
        if (!nsh.setGrantedCapabilities("{ \"network\": { \"hosts\": [\"api.example.com\"] }, \"host\": [\"user.*\", \"analytics\"] }")) {
            throw new IllegalStateException("invalid capability grant");
        }
        nsh.loadAssetBundle("nsh"); // runs assets/nsh/bundle.nshc
    }

    void conversationTapped(String id) {
        nsh.send("conversation.open", Map.of("id", id));
    }

    @Override
    protected void onDestroy() {
        nsh.destroy();
        super.onDestroy();
    }
}
```

In Kotlin (not compiled here), the same calls read `nsh.handle("user.profile") { data, reply -> reply.resolve(mapOf("id" to (data as Map<*, *>)["id"], "name" to "Ada")) }`.

Activity setup, from the [Android guide](https://nsh.dev/docs/android.md#activity-setup):

- **Back.** `NshView` takes back presses by itself while the app wants them. For predictive back on Android 13+, set `android:enableOnBackInvokedCallback="true"` on `<application>`.
- **Keyboard.** Leave `android:windowSoftInputMode` unset or use `adjustResize`. With `adjustPan` the system moves the whole window.
- **Rotation.** An activity recreated on rotation destroys the view, and the app restarts. To keep state, declare the changes yourself: `android:configChanges="orientation|screenSize|screenLayout|smallestScreenSize|keyboard|keyboardHidden|navigation|uiMode|density|fontScale|locale|layoutDirection"`.
- **Lifecycle.** Call `destroy()` when the activity or fragment goes away.
- **Dependencies.** `debugImplementation(files("libs/nsh-dev.aar"))`, and `releaseImplementation(files("libs/nsh.aar"))`.

## macOS and other Rust hosts

A Rust host links your crate and the platform crate, and shows the root with no JavaScript VM involved. Keep the view while the window shows it.

```rust
// src/mac_host.rs
#![cfg(target_os = "macos")]

use std::rc::Rc;

use nsh_platform_macos::{objc2::MainThreadMarker, objc2_app_kit::NSWindow, set_ui_sets, AppOptions, AppView, Root};

pub fn show(window: &NSWindow) -> Rc<AppView> {
    // Once, before the first view: the UI sets the apps import, NSH's and your crate's.
    set_ui_sets(vec![nsh_base::registry(), crate::registry()]);
    let mtm = MainThreadMarker::new().expect("main thread");
    let view = AppView::new(mtm, AppOptions::default());
    // Register native components here, before loading.
    view.load(Root::of::<crate::App>().into());
    window.setContentView(Some(&view.view()));
    view
}
```

GPUI is the same shape, not compiled here because `nsh-platform-gpui` has its own workspace in the NSH repository:

```rust
// fragment
set_ui_sets(vec![nsh_base::registry(), my_app::registry()]);
cx.new(|cx| {
    let view = AppView::new(AppOptions::default(), window, cx);
    view.load(App::Rust(Root::of::<my_app::App>()));
    view
})
```

Put that entity in your element tree like any other view. On GPUI a host can also accept a package only after it proves healthy; see `nsh-signed-delivery`.

## The host channel

The view and its app talk through `host`, with JSON data. It needs no view in the app and no native component.

The NSH side:

```tsx
// App.tsx
import { host, mount, nativeComponent, type NativeComponentRef, signal, Text, View } from "nsh";

/** The host's badge. Only the props listed in the spec are sent to the native side. */
const Badge = nativeComponent<{ text: string }, { tap: { count: number } }>("embed.badge", {
  props: ["text"],
  events: ["tap"],
});

function App() {
  const conversation = signal(String(host.params?.conversation ?? "none"));
  const name = signal("nobody");
  const taps = signal(0);
  let badge: NativeComponentRef | undefined;

  const stop = host.on("conversation.open", (event) => {
    conversation.set((event as { id: string }).id);
  });
  void stop; // Call it to stop listening, as a component's cleanup would.

  const loadProfile = async () => {
    try {
      const user = (await host.call("user.profile", { id: 7 })) as { name: string };
      name.set(user.name);
      host.post("analytics", { screen: "profile" });
    } catch (error) {
      name.set((error as { code: string }).code);
    }
  };

  return (
    <View class="flex-1 gap-3 p-4">
      <Text>{`Conversation ${conversation()}`}</Text>
      <Text>{`Name ${name()}`}</Text>
      <Text role="button" onPress={loadProfile}>
        Profile
      </Text>
      <Badge
        ref={(node) => (badge = node)}
        text={`Tapped ${taps()}`}
        onTap={(e) => {
          taps.set(e.count);
          badge?.command("pulse");
        }}
        class="self-start rounded-lg"
      />
    </View>
  );
}

mount(App);
```

Declare what the app calls, or every call is refused, even with no `"capabilities"` key. The host's functions are never granted by default:

```json
{
  "name": "Orders",
  "id": "dev.example.orders",
  "version": "1.0.0",
  "build": 1,
  "capabilities": { "host": ["user.*", "analytics"] }
}
```

Rules:

- End a prefix with a separator: `"user.*"`. `"user*"` also matches `userAdmin`.
- Names are 1 to 128 characters from letters, digits, `_`, `.`, `:` and `-`, without `..`.
- A call needs a grant from both the app's declaration and, when the host grants at all, the host. Keep both in sync, as the host grants above do.
- `host.available()` is false where nothing embeds the app, so a standalone build needs a fallback.
- A call waits 30 seconds by default. Pass `{ timeout }` as its third argument to change that.
- A reply released unanswered fails the call. Events sent before the app listens are kept, so a tapped notification still arrives.
- Keep the interface to small JSON values, sending identifiers rather than large blobs.

Test the app with the test playing the host, so this runs with no native build at all:

```tsx
// App.test.tsx
import { expect, open, test } from "nsh/test";

test("reads params, calls the host, hears its events", async () => {
  const app = await open("./App.tsx", {
    host: {
      params: { conversation: "c-42" },
      handlers: {
        "user.profile": async ({ id }: { id: number }) => ({ id, name: "Ada" }),
        analytics: null,
      },
    },
  });
  expect(app.texts()).toContain("Conversation c-42");

  await app.press("role:button[Profile]");
  await app.waitFor("text:Name Ada");
  expect(app.host.calls()).toEqual([{ name: "user.profile", data: { id: 7 } }]);
  expect(app.host.posts()).toEqual([{ name: "analytics", data: { screen: "profile" } }]);

  await app.host.send("conversation.open", { id: "c-43" });
  expect(app.shows("text:Conversation c-43")).toBe(true);
});

test("a host with no handler for the name rejects the call", async () => {
  const app = await open("./App.tsx", { host: { handlers: { analytics: null } } });
  await app.press("role:button[Profile]");
  await app.waitFor("text:Name ERR_NSH_HOST_ERROR");
});

test("no host at all is a different failure", async () => {
  const app = await open("./App.tsx");
  await app.press("role:button[Profile]");
  await app.waitFor("text:Name ERR_FEATURE_UNAVAILABLE_ON_PLATFORM");
});
```

What a failed call rejects with, as measured on 0.4.0:

| Situation | `error.code` |
|---|---|
| The name is not in `capabilities.host` | `ERR_ACCESS_DENIED` |
| The name has no handler, or the handler throws | `ERR_NSH_HOST_ERROR` |
| Nothing embeds the app, including `open` with no `host` or with `handlers: {}` | `ERR_FEATURE_UNAVAILABLE_ON_PLATFORM` |
| The answer took too long | `ETIMEDOUT` |

`host.handlers` runs headless only, and `--target` rejects it with `ERR_NSH_TEST_UNSUPPORTED`. So the two halves are only tested together in your host app's own tests, with its real handlers.

## Native components

A native component is a host view the app uses like an element. It lays out, clips and scrolls with the rest, since it is one more subview in the same hierarchy. The `Badge` above is the full shape, on both sides.

| Concern | Rule |
|---|---|
| Naming | Prefix with your company or app: `acme.chart`. Names under `nsh/` are NSH's. |
| Timing | Register before loading, so the first frame finds them. |
| Missing | A name nothing registered renders an empty view, and logs once. |
| Props | JSON only, sent once on creation and again on change. Removed props arrive as `nil` or `null`. |
| Decoration | Background, border, radius and shadow apply to the container NSH draws. |
| Size | Set by the app's layout, or reported with `setSize`. |
| Children | None. A native component is a leaf. |
| Security | **Capabilities do not apply.** It is the host's own code, so register only what is safe for every app that view may run. |
| Web | The web has no registered components. Give TSX a fallback there. |

Register the same names with the same props on iOS and Android, so one bundle runs on both. Rust apps use `native("embed.badge")` with `.prop(..)` and `.on_native_event(..)`. An app with its own crate can register inside it, with a `#[native_components(macos)]` function per platform, so `nsh run` and `nsh package` hosts get them with no host code.

## Theme overrides

One app in several branded hosts takes each host's brand. Overrides use the `nsh.json` `"theme"` format, win over the app's tokens of the same name, and apply at once:

```swift
// Overrides.swift
import NativeShell

@MainActor
func applyBrand(_ app: NshAppView) throws {
    try app.setThemeOverrides("""
    { "cssVars": {
        "theme": { "radius": "0.75rem" },
        "light": { "primary": "#e11d48", "primary-foreground": "#ffffff" },
        "dark": { "primary": "#fb7185", "primary-foreground": "#1c1917" }
    } }
    """)
}
```

This works because classes and bindings that name tokens restyle natively. An app that hard-coded colors does not get the brand, which is one more reason for the `nsh-component-library` rules. To use a token only the host provides, also give it a value in the app's own theme, or the class check reports it as unknown.

## Web

A page is the app's host. `createPlayer` takes a `host` object whose `call(name, args)` answers, and `player.hostEvent(name, data)` sends events. A published site gets its host from `nsh.json`, with `web.host` naming a module that exports `createHost(player)`. Not compiled here:

```js
import { createPlayer } from "./nsh.js";

const player = await createPlayer(document.querySelector("#app"), {
  wasm: "./nsh.wasm",
  assetBase: "./assets/",
  host: {
    async call(name, args) {
      if (name === "user.profile") return { id: args.id, name: "Ada" };
      throw new Error(`no host function ${name}`);
    },
  },
});
await player.runModule(new URL("./bundle.js", location.href).href);
```

Headless tests load `web.host` too, so `--web-host <module>` can test it without a browser. A Rust app embeds as its published site, in an iframe.

## Copy these rules into the project

```markdown
## NSH embedding rules
- Setup order: components, limits and grants, params, handlers, then load. Settings after a load wait for the next one.
- The app's nsh.json lists every host function it calls. The host's grant lists them too.
- Bytecode ships inside the app bundle. Anything from storage is JS source or a verified package.
- Debug links NativeShellDev and nsh-dev. Release links NativeShell and nsh.
- Hosts keep the view alive, and Android calls destroy().
- iOS forwards the status bar overrides to the view. Android declares the configChanges it handles.
- Native components are named with our prefix and exposed only when safe for every app that view may run.
- The TSX side is tested with open()'s host option. The host side is tested in the host app.
- A device run is required before calling an embedding done.
```

## Review checklist

- [ ] Settings come before the load, and handlers exist for every declared host function.
- [ ] `onError` or the error listener reports to the host's logging.
- [ ] Limits and grants are set when the view may run third-party code.
- [ ] Standalone builds handle `host.available()` being false.
- [ ] One bundle registers the same component names on iOS and Android.
- [ ] Release builds do not link the Dev framework.
- [ ] A real device or simulator run was done, or the report says it was not.

## Grounded in

- NSH iOS guide, framework, `NshAppView`, channel, theme overrides: https://nsh.dev/docs/ios.md
- NSH Android guide, `NshView`, channel, activity setup: https://nsh.dev/docs/android.md
- NSH macOS guide, embedding in a Mac app: https://nsh.dev/docs/macos.md#embedding-in-a-mac-app
- NSH GPUI guide, embedding in a GPUI app: https://nsh.dev/docs/gpui.md#embedding-in-a-gpui-app
- NSH Web guide, `createPlayer` and the host's functions: https://nsh.dev/docs/web.md
- NSH Native Components guide, props, events, commands, rules: https://nsh.dev/docs/native-components.md
- NSH Sandbox and capabilities, `host` grants: https://nsh.dev/docs/sandbox-and-capabilities.md
- NSH Testing guide, playing the host app: https://nsh.dev/docs/testing-and-trace.md#playing-the-host-app
- NSH source at v0.4.0, `NativeShell.swift` and `NshView.java`: https://github.com/longbridge/nsh/tree/v0.4.0/crates
