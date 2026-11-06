---
name: nsh-signed-delivery
description: Ship Native Shell (NSH) code to embedded host apps after release using signed packages, covering root and release keys, nsh publish and verify, revocation, rollback protection, capabilities and host grants, VM limits, and the compatibility promise; use when delivering TSX screens or third-party mini apps over the air, setting up signing keys, responding to a leaked key, or reviewing what a downloaded NSH package may do.
---

# NSH signed delivery

A signed package (`.nshpkg`) is how a host app runs NSH code it did not ship with: an update to its own screens, or a third-party mini app. The stakes differ from ordinary app work. The code runs inside your app with whatever you grant it, and a mistake with a root key can only be fixed with an app update. This skill is the working procedure. Every `nsh` command below was run end to end on `nsh` 0.4.0 (e626a8d) on 2026-11-06, and the output shown comes from those runs, while Swift and Java samples were type-checked against the shipped frameworks. No package was loaded on a device, so the device-side checks described here rest on the NSH docs.

## When to Use

- Delivering TSX screens or mini apps to a host app after release.
- Creating, rotating or revoking signing keys.
- Deciding what a package may reach, and what the host grants it.
- A package is refused on a device, or a key has leaked.
- Reviewing a mini-app or over-the-air design for safety.

Packages hold TSX only. A Rust crate ships in the host app, and a Rust app (`"root"` in `nsh.json`) cannot be published: `nsh publish` refuses it and says so. A Rust app can still deliver TSX screens as packages, through a Rust-with-TSX setup. For replacing a whole standalone app, the [official `nsh` skill](https://github.com/longbridge/nsh/tree/main/skills/nsh) describes `nsh/updater` and `nsh update-manifest`. Those are on `main` and **not in 0.4.0**: `nsh update-manifest` is an unrecognized subcommand on this build.

## The trust model

| Thing | Where it lives | If it leaks |
|---|---|---|
| Root secret key | Offline, such as an encrypted drive in a safe | Anyone can certify keys all your hosts trust. Requires an app update |
| Root public key | Inside the host app | Public |
| Release secret key | Build machine or CI secrets | Revoke it, and certify a new one. No app update |
| Release certificate | With the release key; not secret | Public |
| Revocation list | Fetched by the host, separately from the app | Public, and signed |

Hosts trust only the root. A certificate names the release key, the app ids it may sign for, and a time window. So release keys rotate without touching the host. Hosts can trust several roots, which is how a root itself is rotated.

Why NSH insists: the JS can reach everything the app grants, and QuickJS does not validate bytecode, so crafted bytecode can corrupt memory. NSH therefore loads bytecode only from the app's own resources, a connected dev server, and verified packages.

NSH verifies and runs packages. **It does not deliver them.** Downloading, scheduling and storage are the host's job.

## Set up keys

```bash
nsh keygen keys/root
```

```bash
nsh keygen keys/release
```

```text
key 8030a7eb8f70579d: keys/root.secret (keep it secret), keys/root.pub
key a8c8b08365db0600: keys/release.secret (keep it secret), keys/release.pub
```

`.secret` files are created readable by their owner only, and `nsh keygen` refuses to overwrite: `error keys/root.secret exists; keys are never overwritten`. Add `keys/*.secret` to `.gitignore` before the first run, and move the root secret offline right away.

```bash
nsh certify --root keys/root.secret --key keys/release.pub --apps 'dev.example.*' --days 90 -o keys/release.cert
```

```text
keys/release.cert: key a8c8b08365db0600 certified by root 8030a7eb8f70579d for dev.example.* until 90 days from now
```

`--apps` takes `*`, a prefix such as `dev.example.*`, or an exact id, and repeats. The default is 365 days. The docs' checklist says to certify per team or per app family, for a year or less. Use the narrowest pattern that works: a key certified for `*` is a key for everything.

Give the app a real id too. `com.example.*` works, and `nsh publish` warns that it is a placeholder, "as apps with one id share their data".

## Publish and verify

`build` in `nsh.json` must grow with every release, and `nsh publish` refuses without one.

```bash
nsh publish App.tsx --key keys/release.secret --cert keys/release.cert
```

```text
.nsh/out/com.example.quotes-2.nshpkg: com.example.quotes 1.0.0 (build 2), 0 assets, 7 KB, signed by a8c8b08365db0600, for runtime 0.4.0 or newer
```

```bash
nsh verify .nsh/out/com.example.quotes-2.nshpkg --root keys/root.pub
```

```text
valid: com.example.quotes 1.0.0 (build 2), release key a8c8b08365db0600, sha256 ba268a12c14a85b7e3c97acd241c21d9eab61c574eccae8a12e3242ee7c10838, 0 assets, bytecode for this runtime
```

Always pass `--root` to `nsh verify`. Without it, only the package's own signature is checked, which says nothing about whether you trust the signer.

What we saw these checks refuse:

| Attempt | Result on 0.4.0 |
|---|---|
| Verify against a root the signer does not chain to | `error signed by key ..., which is not trusted` |
| Publish with a certificate for other ids | `error the release key's certificate does not cover this package: it does not cover app ...` |
| One byte changed in the package | `error the signature does not verify` |
| Verify with a list that revokes the package | `error revoked: package ...` |
| Verify with a list that revokes the key | `error revoked: release key ...` |
| Publish with no build number | `error a package needs a build number ...` |
| `--min-runtime` newer than the CLI | `error minimum runtime 9.9.9 is newer than this runtime, 0.4.0` |

A device checks, in order: the certificate against a trusted root, the release key is not revoked, the package signature, the certificate covers the app id and signing time, the minimum runtime, the file table and hashes, the package is not revoked, and the build is not older than the highest accepted. A refused package raises an error naming the check, and **the running app keeps running**.

| Publish flag | Use |
|---|---|
| `--min-runtime <version>` | Runs on older hosts. The default is this CLI's version, so older hosts refuse the package. Test on that host, because a missing import fails at load. Also `minRuntime` in `nsh.json`. |
| `--source-map file` | Keeps your sources out of the package, for public or third-party delivery. Keep the map next to the release, then use `nsh symbolicate`. |
| `--source-map none` | No map at all, and no way to read mapped stacks. |
| `--strip-debug` | Smaller, but stacks have no positions, so nothing can be mapped. |

The default, `--source-map package`, ships your original source inside the package, and is often its largest file. Decide this deliberately.

## Load it in the host

Verification hashes the file and extracts assets, so it runs off the UI thread. Running happens on it.

```swift
// Delivery.swift
import Foundation
import NativeShell

@MainActor
func openDownloaded(_ downloaded: URL, in app: NshAppView, rootPublicKey: String, revocations: String?) async {
    do {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        // Its state, the newest revocation list and the highest build per app, persists here.
        let trust = try NshTrust(stateDirectory: support.appendingPathComponent("nsh"))
        try trust.addRoot(rootPublicKey) // the contents of root.pub, bundled with the app
        if let revocations { try trust.updateRevocations(revocations) }

        // Set these before loading: they apply from the next load.
        app.limits = NshLimits(memoryMB: 128, callBudget: 1, maxNodes: 50_000)
        try app.grantCapabilities(#"{ "network": { "hosts": ["api.example.com"] } }"#)

        let info = try await app.loadPackage(at: downloaded, trust: trust)
        print("running \(info.id) \(info.version) (build \(info.build))")
    } catch {
        print("refused: \(error)") // The running app stays.
    }
}
```

```java
// dev/example/delivery/Delivery.java
package dev.example.delivery;

import android.app.Activity;
import android.util.Log;

import com.longbridge.nsh.NshPackage;
import com.longbridge.nsh.NshTrust;
import com.longbridge.nsh.NshView;

import java.io.File;

final class Delivery {
    /** Call off the UI thread. */
    static void open(Activity activity, NshView nsh, File downloaded, String rootPublicKey, String revocations) {
        NshTrust trust = new NshTrust(new File(activity.getFilesDir(), "nsh"));
        // These report failure by returning false, so a bad key must not pass silently.
        if (!trust.addRoot(rootPublicKey)) throw new IllegalStateException("invalid root key");
        if (revocations != null && !trust.updateRevocations(revocations)) Log.w("nsh", "revocation list not applied");
        try {
            NshPackage pkg = trust.openPackage(downloaded);
            activity.runOnUiThread(() -> {
                nsh.setLimits(128, 0, 0, 1000, 50_000);
                nsh.setGrantedCapabilities("{ \"network\": { \"hosts\": [\"api.example.com\"] } }");
                nsh.runPackage(pkg);
            });
        } catch (NshPackage.RefusedException refused) {
            Log.e("nsh", "refused: " + refused.getMessage()); // The running app stays.
        }
    }
}
```

Each package's storage is kept under the id its release key's certificate vouches for, so one app cannot read another's by claiming its id. A package runs once. Load again for another run.

**A catch in acceptance.** On iOS and Android, `openPackage` verifies and accepts at once, so the new build number is recorded before you know the package works. On GPUI, the host can verify first and accept only after the first frame and a quiet period, which allows going back to the previous build. If a bad release must not strand users on those platforms, keep a known-good older build within your own delivery logic and ship fixes forward with a higher `build`, since older builds stay refused once a newer one was accepted.

## Capabilities: declare, then grant

A package can do only what it declares, and only what the host grants on top. A request must pass both.

```json
{
  "name": "Quotes",
  "id": "dev.example.quotes",
  "version": "1.0.0",
  "build": 3,
  "capabilities": {
    "network": {
      "hosts": ["*.cdn.example.com"],
      "http": [{ "host": "quotes.example.com", "methods": ["GET"], "pathPrefix": "/v1/" }]
    },
    "clipboard": true,
    "host": ["user.*"],
    "openUrl": { "schemes": ["https", "mailto"] }
  }
}
```

Known top-level keys on 0.4.0, from the build's own error text: `network`, `webview`, `statusBar`, `clipboard`, `picker`, `speech`, `openUrl`, `process`, `fs`, `host`.

Traps:

- **No `"capabilities"` key means everything is allowed,** apart from `network.sockets`, `host` and `webview.input`, which must always be named. A mini app you do not trust must declare, and you must review it, because the declaration is what gets signed.
- **A typo fails `nsh build` and passes `nsh check`.** We confirmed that: `nsh check` exited 0, and `nsh build` failed with `nsh.json: capabilities: unknown key 'netwrok' (known: network, webview, ...)`.
- **`*.example.com` does not match `example.com`.** List both when you need both.
- **Grant at the host for third parties.** Without a grant, what each app declares applies, so a package declaring `"hosts": ["*"]` gets everything.
- **Native components are outside all of this.** A registered component is the host's own code, so do not register a sensitive one on a view that runs third-party packages.
- **A Rust crate's own native code is not sandboxed.** Only its calls into NSH's services are checked.
- **A page's subresources are the web content's business.** `webview.hosts` checks top-level navigation only.

A refusal rejects with `code` `ERR_ACCESS_DENIED`, `permission` and `resource`, and logs which key to add. Headless tests enforce all this, so test refusals:

```tsx
// App.tsx
import { Clipboard, mount, signal, Text, View } from "nsh";

function App() {
  const result = signal("idle");
  const report = (error: unknown) => {
    const refused = error as { code?: string; permission?: string };
    result.set(`${refused.code} ${refused.permission}`);
  };
  const load = () =>
    fetch("https://not-granted.example.org/x").then((r) => result.set(`status ${r.status}`), report);
  const copy = () => Clipboard.writeText("hi").then(() => result.set("copied"), report);
  return (
    <View class="flex-1 gap-3 p-4">
      <Text role="button" onPress={load}>
        Load
      </Text>
      <Text role="button" onPress={copy}>
        Copy
      </Text>
      <Text>{result()}</Text>
    </View>
  );
}

mount(App);
```

```tsx
// App.test.tsx
import { open, test } from "nsh/test";

test("a host the app did not declare is refused, even with a stub route", async () => {
  const app = await open("./App.tsx", {
    network: { "GET https://not-granted.example.org/*": { body: "x" } },
  });
  await app.press("role:button[Load]");
  await app.waitFor("text:ERR_ACCESS_DENIED network");
});

test("an undeclared capability is refused", async () => {
  const app = await open("./App.tsx");
  await app.press("role:button[Copy]");
  await app.waitFor("text:ERR_ACCESS_DENIED clipboard");
});
```

## Limits and hardening

An embedded VM runs under limits the host sets per view. They apply from the next load.

| Limit | Default | When hit |
|---|---|---|
| JS heap | 256 MB | The VM stops |
| Interpreter stack | 4096 KB, also the maximum | `RangeError`, which the app can catch |
| Bundle evaluation | 10 s | Evaluation is cut off |
| Any other call into JS | 2 s | That call is cut off; the app goes on |
| Nodes kept | 200 000 | The VM stops |
| Queued operations | 8 000 000 values | The VM stops |
| One canvas drawing | 1 000 000 operations | The VM stops |

Time is budgeted per call, not per app, so a long-running app accumulates nothing. When a VM stops, the error reaches the host's handler once, no more JS runs, and the views stay as they were. So the host needs a plan for a frozen screen, such as replacing the view.

Rehearse under a host's limits with `nsh run App.tsx --limits memory=128MB,call=500ms`.

Release builds are hardened: `eval` and the `Function` constructors are gone, and shared prototypes are frozen. Bundles from `nsh dev` and `nsh run`, even with `--release`, are **not** hardened. So use `nsh test`, which hardens, or a packaged build, to catch a library that relies on dynamic code.

## Revoke

Lists are signed by the root and numbered, and a store accepts only a higher number, so an old list cannot be replayed to un-revoke something.

```bash
nsh revoke --root keys/root.secret --sequence 1 --package ba268a12c14a85b7e3c97acd241c21d9eab61c574eccae8a12e3242ee7c10838 -o revocations.json
```

```text
revocations.json: list 1, 0 release keys and 1 packages revoked
```

```bash
nsh revoke --root keys/root.secret --extend revocations.json --key a8c8b08365db0600 -o revocations-2.json
```

```text
revocations-2.json: list 2, 1 release keys and 1 packages revoked
```

Always `--extend` the last published list, or you drop earlier entries. Publish the newest list where hosts fetch it, ship it separately from the app, and apply it with `updateRevocations` **before** opening packages. A host that never fetches a list never learns of a revocation, so build the fetch in from the first release.

### If a release key leaks

1. Get the root secret from its offline location.
2. `nsh revoke --root keys/root.secret --extend <last list> --key <leaked id> -o <next list>`, then publish the list.
3. `nsh keygen keys/release-2`, then `nsh certify` it, narrowly.
4. Raise `build`, publish again with the new key, and verify.
5. Rotate the CI secret, and find out how it leaked.

Revoking a key refuses every package it signed, including good ones, so hosts need the re-signed release promptly. Have the new package ready before publishing the list.

## A platform that signs for others

CI signs with a key only test apps trust, and the platform signs again with the key production apps trust. The content stays the same, and the certificate, signing time and signature change. So the package gets a new SHA-256, which is the one to revoke.

```bash
nsh publish ci.nshpkg --key keys/platform.secret --cert keys/platform.cert -o release.nshpkg
```

```text
release.nshpkg: com.example.quotes 1.0.0 (build 4), signed again by f8428486447c5efa, for runtime 0.4.0 or newer
```

Signing again vouches for the package's bytecode, so sign again only what your own builds made. For third-party code, review the source and build it yourself.

## Compatibility

NSH's [compatibility promise](https://nsh.dev/docs/compatibility.md) is that an app built for a compatible release runs on every later release, unchanged. It is enforced by frozen records in a separate repository that NSH's pull requests cannot edit. For you:

- A package from a **newer** `nsh publish` runs on a host whose runtime meets its minimum. What the runtime does not know is left out, and it never widens what the package may do. Unknown capabilities are not granted, and a grant with an unknown key is dropped whole, because that key might restrict it.
- Types only widen, so a `switch` on `platform.os` or a state needs a `default`.
- Between releases, an API that only `main` has may still change. Do not ship against `main`.
- The site follows the latest release, and a host in the field may be older. Pin `--min-runtime` deliberately, and test on that runtime.

## Copy these rules into the project

```markdown
## NSH delivery rules
- root.secret is offline and never in CI. Release keys are in CI secrets. *.secret is git-ignored.
- Certificates use the narrowest --apps pattern, for a year or less.
- Every release raises build, and runs `nsh verify --root root.pub` before upload.
- Hosts fetch and apply the newest revocation list before opening packages, from the first release.
- Third-party packages declare capabilities, we review them, and the host grants at most that, with limits.
- No sensitive native component on a view that runs third-party code.
- Revocation lists always --extend the last published one. A replacement build is ready first.
- `--source-map` is chosen deliberately. The default ships original sources in the package.
- Test releases with nsh test or a packaged build, since nsh run is not hardened.
- A package is tried on the oldest runtime it claims with --min-runtime.
```

## Review checklist

- [ ] The root secret is offline, and no secret key is in git.
- [ ] The host bundles only `root.pub`, and calls `addRoot` before opening.
- [ ] Verification runs off the UI thread, and a refusal leaves the running app in place.
- [ ] Revocation lists are fetched and applied before opening.
- [ ] Limits and grants are set before the load, for anything third-party.
- [ ] The host handles a VM that stopped, through `onError` or the error listener.
- [ ] Capability declarations were reviewed, not assumed, and refusals are tested.
- [ ] There is a written procedure for a leaked key, and a way to ship a fix fast.

## Grounded in

- NSH Signed Packages, keys, publish, verify, revoke, rollback, re-signing, checklist: https://nsh.dev/docs/signed-packages.md
- NSH Sandbox and Capabilities, limits, hardening, capabilities, grants, refusals: https://nsh.dev/docs/sandbox-and-capabilities.md
- NSH Backward Compatibility, the promise and how it is enforced: https://nsh.dev/docs/compatibility.md
- NSH CLI reference, signing commands: https://nsh.dev/docs/cli.md
- NSH iOS guide, `NshTrust` and `NshLimits`: https://nsh.dev/docs/ios.md#signed-packages
- NSH Android guide, `NshTrust`, `setLimits`: https://nsh.dev/docs/android.md#signed-packages
- NSH GPUI guide, verify first and accept later: https://nsh.dev/docs/gpui.md#embedding-in-a-gpui-app
- NSH source at v0.4.0, `NativeShell.swift` and `NshTrust.java`: https://github.com/longbridge/nsh/tree/v0.4.0/crates
- longbridge/nsh-compat, the frozen compatibility records: https://github.com/longbridge/nsh-compat
