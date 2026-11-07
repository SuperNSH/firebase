// Build nsh-firebase — one fresh workflow per docs/PROMPT.md.
// Eleven areas run their own implement→audit→tests(→device/vision) chains in one fan-out
// with zero global barriers; then milestone docs + whole-tree smoke (until zero issues, cap 3);
// stretch areas run only if satisfaction criteria 1-8 hold. Every subagent is GLM-5.3-Flash$high.

const MODELS = { flash: model("account:zai-individual-coding-plan/GLM-5.3-Flash$high") };
const KIT = "/Users/hmziq/ZCodeProject/ai/harness/zcode/agents";

// ---------- small helpers ----------
function deferred(): { promise: Promise<any>; resolve: (v: any) => void; reject: (e: any) => void } {
  let resolve!: (v: any) => void;
  let reject!: (e: any) => void;
  const promise = new Promise((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}
function raceTimeout(p: Promise<any>, ms: number, label: string): Promise<any> {
  let t: any = null;
  const timer = new Promise((_res, rej) => { t = setTimeout(() => rej(new Error("waited too long for " + label)), ms); });
  return Promise.race([p, timer]).finally(() => clearTimeout(t));
}
function asJson(r: any): any {
  if (r == null) return {};
  if (typeof r === "object") return r;
  const s = String(r);
  const fence = s.match(/```(?:json)?\s*([\s\S]*?)```/);
  const body = fence ? fence[1] : s.slice(s.indexOf("{"), s.lastIndexOf("}") + 1);
  try { return JSON.parse(body); } catch { return { raw: s }; }
}
function outStr(r: any): string {
  if (r == null) return "";
  return String(r.stdout ?? r.output ?? r.text ?? "") + "\n" + String(r.stderr ?? r.error ?? "");
}
function gateOk(r: any): boolean {
  if (r == null) return false;
  if (typeof r.ok === "boolean") return r.ok;
  if (typeof r.exitCode === "number") return r.exitCode === 0;
  if (typeof r.code === "number") return r.code === 0;
  return false;
}

// ---------- specialist personas: runtime read, never pasted ----------
async function specialist(name: string, fallback: string): Promise<{ persona: string; source: string }> {
  const adapter =
    `You are the ${name} specialist for one area of a parallel workflow. ` +
    `Your full specialist contract follows verbatim — apply it to this area's work; ` +
    `where it conflicts with your ask, the ask wins.` + "\n\n";
  for (const p of [`${ROOT}/.zcode/agents/${name}.md`, `${KIT}/${name}.md`]) {
    try {
      const raw = await files.read(p);
      return { persona: adapter + raw.replace(/^---\n[\s\S]*?\n---\n/, "").trim(), source: p };
    } catch { /* try next source */ }
  }
  return { persona: fallback, source: "fallback" };
}

// ---------- live board ----------
artifact.board("areas", {
  key: "name",
  status: "phase",
  columns: ["implementing", "auditing", "testing", "verifying", "fixing", "done", "failed"],
  notes: "summary",
});
function board(name: string, phaseName: string, summary: string) {
  try { report({ name, phase: phaseName, summary }, "areas"); } catch { /* board updates never fail an area */ }
}

// ---------- hard rules that travel into every ask (from docs/PROMPT.md) ----------
const RULES = `
HARD RULES (docs/PROMPT.md binds every agent in this repo — read it first):
- Never invent an API. Verify every name in ~/fo/.refs/nsh-v0.4.0 source (what is installed), in ~/.cargo/registry/src/*/nsh-core-0.4.0 for Rust, in .nsh/types after \`nsh check\` for TSX, or in ~/fo/nsh-query and ~/fo/nsh-form (read-only). The upstream nsh skill and nsh.dev docs follow main and are ahead of 0.4.0: no \`nsh update-manifest\`, no \`nsh/updater\`, no Notifications module, no \`notifications\` capability on 0.4.0.
- Rust gate: \`cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test\` (scoped with -p while your area works). TSX gate: \`nsh check\` && \`nsh build\` with every warning fixed && \`nsh test\`. No check is weakened, skipped or deleted.
- Every new test is seen to fail once, by breaking the code on purpose; confirm the failure names the right thing. Load the test-quality-guardrails skill.
- Unit tests run headless through the UI on a manual clock (\`clock: {}\` / \`Clock::manual\`) with stubbed network and no real time. Sandbox tests that touch the emulator suite or 127.0.0.1 sinks live in separate, labelled files (\`#[ignore = "sandbox: needs emulator suite"]\` for Rust; \`tests/emulator/*.test.tsx\` for TSX) and are run by the repo's runner scripts only. A headless pass proves behavior, never pixels.
- Comments are 1-2 lines stating a non-obvious constraint; never narrate the next line. Dead code is deleted. No speculative abstractions.
- No real accounts, no real credentials, no billing. No real google-services.json or GoogleService-Info.plist — fake fixtures with obviously fake values live under fixtures/. No publish, no push. Do NOT run \`git commit\` yourself: after your chain passes, report your file list and the workflow's committer commits it.
- This Mac is SHARED (nsh-starter, nsh-http and nsh-auth are building at the same time): stay inside /Users/hmziq/fo/nsh-firebase; treat ~/fo/.refs, ~/fo/nsh-query, ~/fo/nsh-form as read-only; devices are named after this repository; \`adb -s <serial>\` on every Android call; never run \`xcrun simctl shutdown all\`, \`erase all\`, \`adb kill-server\`, \`killall\`, or stop a process you did not start; pick free ports; prefer non-screen-taking capture (\`xcrun simctl io <udid> screenshot\`, \`adb -s <serial> exec-out screencap -p\`, \`nsh test --target ... \` with \`app.screenshot(path)\`).
- Evidence: PNGs and logs go under docs/evidence/<area>/ and every PNG must actually be looked at. Record done / verified-how / not-verified in docs/status/<area>.md (only your own file). A claim with no evidence is reported as unverified — never fake a pass.
- Work directly in the current checkout: no branches, no worktrees, no history rewriting. Touch ONLY the files your area owns. No whole-tree test runs (the milestone does that once). Finish your piece and report.
- 0.4.0 traps: \`nsh check\` needs an App.tsx at the package root or an entry passed to it; \`If\`/\`Each\` are globals in TSX (importing them is a type error); dev-dependencies must be 0.4.0 (the docs' \`nsh-engine = "0.1"\` samples are stale); the generated Cargo.toml has no \`log\` crate; \`Headless::start\` can return before the first frame so \`wait_for\` something first; \`Signal::update\` needs \`T: Clone\`; a component's signals are disposed when its \`cleanup\` runs (capture what cleanup needs in an Rc<Cell<..>>); host handler parameters need annotations (\`async ({ id }: { id: number }) => ...\`); selector regexes use Rust's \`regex\` (no look-ahead); a TSX app consumes a sibling package via \`mkdir -p node_modules && ln -s <path> node_modules/<name>\`; failed host calls reject with ERR_ACCESS_DENIED (undeclared), ERR_NSH_HOST_ERROR (unhandled or thrown), ERR_FEATURE_UNAVAILABLE_ON_PLATFORM (no host); bytecode loads only from an app bundle, APK or verified package.
Reply to every ask with ONLY this JSON (no prose around it):
{"done": <bool>, "summary": "<what you did, files touched, commands run>", "contractLine": "<one sentence copied verbatim from the specialist contract above you that most shaped the work>", "blockers": ["<anything that stopped you>"], "evidence": "<paths of evidence files you wrote, or empty>"}
`;

const REPORT_FORMAT = `{"done": <bool>, "summary": "...", "contractLine": "...", "blockers": [...], "evidence": "..."}`;

// ---------- serialized shared resources: committer, iOS device, Android device ----------
const committer = agent("repo committer", {
  system:
    "You commit finished work in /Users/hmziq/fo/nsh-firebase. You receive a file list (entries may be descriptive labels like 'Cargo.toml (workspace root)', 'crates/nsh-firebase-{auth,database} stubs', or globs like 'packages/nsh-firebase/tests/core*') and a Conventional Commits message. " +
    "Map each entry to the real paths that exist (a label's parentheses are a comment; braces expand; a directory adds that directory), then run `git add -- <paths...>` and `git commit -m <message>` on branch main. Never `git add -A`, never push, never amend, never touch unrelated files; skip entries with no existing path and note them. " +
    "If the working tree contains other areas' uncommitted files, leave them alone. " +
    "If identity is missing or the commit fails, report it; do not invent an identity. " +
    "Reply with ONLY: {\"done\": <bool>, \"summary\": \"<commit hash or the exact error>\"}.",
  model: MODELS.flash,
});
let commitQ: Promise<any> = Promise.resolve();
function commitRequest(files: string[], message: string): Promise<any> {
  const p = commitQ.then(() =>
    ask(committer, "Commit these files in /Users/hmziq/fo/nsh-firebase (main, Conventional Commits):\nFILES: " + files.join("\n") + "\nMESSAGE: " + message + "\nReply JSON only: {\"done\":bool,\"summary\":\"<hash|error>\"}")
  );
  commitQ = p.then(() => {}, () => {});
  return p;
}

const iOSVerifier = agent("device verifier·iOS", {
  system: (await specialist("z-native-device-verifier", "You verify iOS work on this Mac's simulators using xcrun simctl, xcodebuild, idb and mobilecli; capture without taking over the screen; never touch devices you did not create.")).persona,
  model: MODELS.flash,
});
const androidVerifier = agent("device verifier·Android", {
  system: (await specialist("z-native-device-verifier", "You verify Android work on this Mac's emulators using adb, Gradle and mobilecli; capture without taking over the screen; never touch devices you did not create.")).persona,
  model: MODELS.flash,
});
let iosQ: Promise<any> = Promise.resolve();
function askIOS(task: string): Promise<any> {
  const p = iosQ.then(() => ask(iOSVerifier, task));
  iosQ = p.then(() => {}, () => {});
  return p;
}
let androidQ: Promise<any> = Promise.resolve();
function askAndroid(task: string): Promise<any> {
  const p = androidQ.then(() => ask(androidVerifier, task));
  androidQ = p.then(() => {}, () => {});
  return p;
}

// ---------- cross-area handoffs (resolve inside areas, never block the whole run) ----------
const researchDone = deferred(); // native-tier API choices wait for docs/research.md
const coreDone = deferred();     // core crate + package skeleton + emulator suite wait
const demoShellReady = deferred();  // perfmon's monitoring screen waits for the demo shell
const demoBundleReady = deferred(); // native device phases wait for the built demo bundle
const implDone: Record<string, { promise: Promise<any>; resolve: (v: any) => void; reject: (e: any) => void }> = {};
function implFlag(name: string) {
  if (!implDone[name]) implDone[name] = deferred();
  return implDone[name];
}

// ---------- gate helpers (scoped per area; whole-tree runs only at the milestone) ----------
type Spec = { label: string; cmd: string; args: string[]; timeoutMs: number };
async function runSpecs(specs: Spec[]): Promise<{ ok: boolean; failures: { label: string; out: string }[] }> {
  const failures: { label: string; out: string }[] = [];
  for (const s of specs) {
    let r = await world.run(s.cmd, s.args, { timeoutMs: s.timeoutMs });
    // A long cold build can outlive one window; the incremental retry finishes it.
    if (!gateOk(r) && /timed out|timeout/i.test(outStr(r))) r = await world.run(s.cmd, s.args, { timeoutMs: s.timeoutMs });
    if (!gateOk(r)) failures.push({ label: s.label, out: outStr(r).slice(-4000) });
  }
  return { ok: failures.length === 0, failures };
}

// ---------- per-area verification loop: audit until zero issues (cap 3), then gates (cap 3) ----------
async function auditLoop(a: AreaDef, impl: any): Promise<{ converged: boolean; rounds: number; issues: string[] }> {
  let issues: string[] = [];
  for (let round = 1; round <= 3; round++) {
    board(a.name, "auditing", a.short);
    const checker = agent(`${a.short} auditor·${round}`, { system: a.persona, model: MODELS.flash });
    const res = asJson(await ask(checker,
      "AUDIT (read-only — do not edit anything) the files this area owns: " + a.owns.join(", ") + "\n" +
      "Requirements: " + a.auditFocus + "\n" +
      "Check the work against docs/PROMPT.md's hard rules for these files: gates would pass (read the code, do not run whole-tree commands; you may run the area's scoped read-only inspections), APIs are real (verified against 0.4.0 source, .nsh/types or the reference repos), comments follow the 1-2-line rule, no secrets, no invented names, tests were built to fail-first and use manual clocks with stubbed network. " +
      "Return ONLY actionable findings a fixer can act on — no PASS notes, no praise. " +
      "Reply JSON only: {\"done\":bool,\"issues\":[{\"file\":\"<path>\",\"note\":\"<what is wrong and the exact fix>\"}],\"summary\":\"...\"}"));
    issues = Array.isArray(res.issues) ? res.issues.map((i: any) => (i && i.file ? i.file + ": " + i.note : String(i))) : [];
    if (issues.length === 0) return { converged: true, rounds: round, issues };
    board(a.name, "fixing", "audit findings");
    await ask(impl, "Your auditor found these actionable issues in the files you own. Fix every one; touch nothing outside your owned files; re-run your scoped checks; keep every gate green.\nFINDINGS:\n- " + issues.join("\n- ") + "\n" + "Reply JSON only: " + REPORT_FORMAT);
  }
  return { converged: false, rounds: 3, issues };
}

async function gateLoop(a: AreaDef, impl: any): Promise<{ converged: boolean; rounds: number; failures: string[] }> {
  let failures: string[] = [];
  for (let round = 1; round <= 3; round++) {
    board(a.name, "testing", a.short);
    const res = await runSpecs(a.gates);
    if (res.ok) return { converged: true, rounds: round, failures };
    failures = res.failures.map((f) => f.label);
    board(a.name, "fixing", "gate failures");
    await ask(impl,
      "Your area's scoped gates failed. Fix the causes in the files you own; never weaken, skip or delete a check; keep new tests seen-to-fail-once honest.\n" +
      res.failures.map((f) => "GATE " + f.label + " FAILED, tail of output:\n" + f.out).join("\n\n") +
      "\nReply JSON only: " + REPORT_FORMAT);
  }
  return { converged: false, rounds: 3, failures };
}

// ---------- the area shape ----------
type AreaDef = {
  name: string; short: string; personaName: string | null; persona: string; owns: string[];
  auditFocus: string; gates: Spec[];
  mid?: (a: AreaDef, impl: any) => Promise<string>; // extra work between implement and audit (handoff-driven)
  run: (a: AreaDef, impl: any) => Promise<{ criteria?: Record<string, string>; extra?: Record<string, any> }>;
};

async function runArea(a: AreaDef): Promise<any> {
  try {
    board(a.name, "implementing", a.short);
    const spec = a.personaName ? await specialist(a.personaName, "You are a careful, honest implementer.") : { persona: "You are a careful, honest implementer.", source: "fallback" };
    a.persona = spec.persona;
    const impl = agent(`${a.personaName ?? "implementer"}·${a.name}`, { system: a.persona, model: MODELS.flash });
    const res = asJson(await ask(impl, a.implementAsk()));
    implFlag(a.name).resolve(res);
    if (res.contractLine && a.persona.includes(String(res.contractLine).trim()) === false) {
      const again = asJson(await ask(impl, "Copy one sentence VERBATIM from the specialist contract in your system prompt — the sentence that most shaped your work. Reply JSON only: {\"done\":bool,\"summary\":\"...\",\"contractLine\":\"...\"}"));
      if (!again.contractLine || a.persona.includes(String(again.contractLine).trim()) === false) {
        res.contractLine = "(persona delivery unverified)";
      }
    }
    let midNote = "";
    if (a.mid) { board(a.name, "implementing", a.short + " (second pass)"); midNote = await a.mid(a, impl); }
    const audit = await auditLoop(a, impl);
    const gates = await gateLoop(a, impl);
    let extra: Record<string, any> = {};
    let criteria: Record<string, string> = {};
    try {
      const devRes = await a.run(a, impl);
      criteria = devRes.criteria ?? {};
      extra = devRes.extra ?? {};
    } catch (e: any) {
      extra.deviceVisionError = String(e && e.message ? e.message : e);
    }
    if (midNote) extra.mid = midNote;
    const files = [...a.owns, "docs/status/" + a.name + ".md", "docs/evidence/" + a.name];
    const cm = asJson(await commitRequest(files, a.commitMessage).catch(() => ({ done: false, summary: "commit queue lost" })));
    const done = res.done !== false && audit.converged && gates.converged;
    board(a.name, done ? "done" : "failed", String(res.summary ?? "").slice(0, 120));
    return {
      name: a.name, status: done ? "done" : "failed",
      summary: String(res.summary ?? "") + (audit.converged ? "" : " | AUDIT DID NOT CONVERGE (cap 3): " + audit.issues.slice(0, 3).join("; ")) + (gates.converged ? "" : " | GATES DID NOT CONVERGE (cap 3): " + failuresStr(gates.failures)),
      blockers: res.blockers ?? [], evidence: res.evidence ?? "", criteria, extra,
      auditRounds: audit.rounds, gateRounds: gates.rounds, commit: cm.summary ?? "", personaSource: spec.source,
    };
  } catch (e: any) {
    implFlag(a.name).reject(e);
    board(a.name, "failed", "area error");
    return { name: a.name, status: "failed", summary: "area error: " + String(e && e.message ? e.message : e), blockers: [String(e)], criteria: {}, extra: {} };
  }
}
function failuresStr(fs: string[]): string { return fs.slice(0, 4).join("; "); }

// =====================================================================================
// phase 1 — the fan-out
// =====================================================================================
phase("Research, core and every product area run their own implement-audit-test chains in parallel — no area waits for another");

const areas: AreaDef[] = [];

// ---------- 0. research ----------
areas.push({
  name: "research",
  short: "research",
  personaName: null,
  persona: "",
  owns: ["docs/research.md", "docs/upstream-issues.md"],
  commitMessage: "docs(research): two-tier survey of RNFB, FlutterFire and the Firebase REST surfaces",
  auditFocus:
    "docs/research.md covers every product (Core, Auth, Realtime Database, Cloud Storage, Messaging+notifications, Analytics, Performance) on every target (pure tier over fetch/WebSocket for nsh-package apps, desktop and web; iOS native; Android native) with an honest what-cannot-be-done note per cell; it cites both ~/fo/.refs/react-native-firebase and ~/fo/.refs/flutterfire by file path (spot-check that at least five cited paths exist); official REST docs cited by URL (Identity Toolkit + Secure Token, RTDB REST with Server-Sent Events, Cloud Storage resumable REST, GA4 Measurement Protocol); the two-tier table from PROMPT.md is tested against the evidence and any change is justified; the unreleased Notifications module names are documented from nsh-main source; docs/upstream-issues.md lists any nsh-query/nsh-form bugs with workarounds (may be an honest 'none found').",
  gates: [],
  implementAsk: () => `
You are the research area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Everything else (code, hosts, demo) is other areas' work — you write exactly two files and nothing else.
${RULES}
YOUR FILES: docs/research.md and docs/upstream-issues.md.
TASK: Read /Users/hmziq/fo/nsh-firebase/docs/PROMPT.md in full, then study and write docs/research.md:
1. Study ~/fo/.refs/react-native-firebase by file path: packages/app (RNFBApp.podspec, firebase_json.rb, firebase_spm.rb for iOS SDK pinning; android/build.gradle + firebase-json.gradle for the Android BoM; plugin/src/{android,ios} for config-file wiring), packages/messaging (ios/RNFBMessaging/RNFBMessaging+{AppDelegate,FIRMessagingDelegate,UNUserNotificationCenter}.* delegate takeover; android .../ReactNativeFirebaseMessagingService.java, HeadlessService, Receiver, Store for background delivery), packages/perf (lib/HttpMetric.ts, android UniversalFirebasePerfModule.java) and the auth/database/storage/analytics packages' lib/ for the JS API shapes.
2. Study ~/fo/.refs/flutterfire by file path: the federated layout (firebase_x, firebase_x_platform_interface, firebase_x_web), firebase_core/firebase_core/windows (C++ SDK path), ios/firebase_*/Package.swift + podspec pairs (both Apple package managers), Pigeon-generated messaging, firebase_messaging background isolates and token refresh.
3. The Firebase REST surfaces, with current official docs found via web search and cited by URL: Identity Toolkit (accounts:signUp, accounts:signInWithPassword, accounts:signInWithCustomToken) and Secure Token (token refresh grant_type=refresh_token); Realtime Database REST with SSE live listeners (EventSource format, path queries, shallow, auth via idToken); Cloud Storage resumable upload REST (X-Upload-Content-Length, upload_id resume, download tokens); GA4 Measurement Protocol (api_secret limits, what it cannot see vs the native pipeline).
4. Answer per product per target: the honest path, and what cannot be done. Test PROMPT.md's two-tier shape against this evidence and write down why you agree or change it.
5. Document the unreleased Notifications module names from ~/fo/.refs/nsh-main (crates/nsh-core/src/notifications.rs and website/modules/nsh.md section Notifications): permission(), requestPermission(), show({id,title,body,silent}), close(id), setBadge(n), on("press") — the messaging facade must match these so moving to the released module later is one small change.
6. docs/upstream-issues.md: skim the READMEs and APIs of ~/fo/nsh-query and ~/fo/nsh-form; record any bug or gap this project must work around, or an honest "none found".
Do not edit any other file. Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => {
    researchDone.resolve({ ok: true });
    return {};
  },
});

// ---------- 1. core ----------
areas.push({
  name: "core",
  short: "core",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: [
    "Cargo.toml (workspace root)", "rustfmt via workspace", "package.json (root)", "scripts/", ".gitignore (append only)",
    "emulator/", "crates/nsh-firebase-core/", "crates/nsh-firebase-{auth,database,storage,messaging,analytics,perf,firestore,remote-config,crashlytics,installations,app-check,functions} stubs",
    "packages/nsh-firebase/{package.json,tsconfig.json,nsh.json,App.tsx,src/index.ts}", "packages/nsh-firebase/src/core/", "packages/nsh-firebase/tests/core*",
    "packages/nsh-firebase/src/{auth,database,storage,messaging,analytics,perf,monitor,firestore,remote-config,crashlytics,installations,app-check,functions}/ stub files",
  ],
  commitMessage: "feat(core): workspace, two-tier facade, error type, event bus and the demo-nsh emulator suite",
  auditFocus:
    "Workspace Cargo.toml declares every crate member (13 firebase crates + demo-rs) with stub crates that compile; packages/nsh-firebase package.json exports map pre-declares every subpath with stub files; the core crate and src/core implement FirebaseOptions/config loading, emulator switches on non-default ports (auth 19099, database 19000, storage 19199, hub 14400), the shared FirebaseError type, the event bus, and the tier facade (Rust trait over the host channel; TS facade over host.call/host.post/host.on with capabilities.host names firebase.*), mapping ERR_ACCESS_DENIED / ERR_NSH_HOST_ERROR / ERR_FEATURE_UNAVAILABLE_ON_PLATFORM honestly; scripts/ gate runners exist and are strict (every nsh build warning fails the gate); emulator/firebase.json + rules + .firebaserc use project demo-nsh with no credentials; no real keys anywhere.",
  gates: [
    { label: "crate core", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-core"], timeoutMs: 600000 },
    { label: "ts core", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/core-app.tsx", "tests/core.test.tsx"], timeoutMs: 600000 },
    { label: "workspace loads", cmd: "cargo", args: ["metadata", "--format-version", "1", "--no-deps"], timeoutMs: 120000 },
    { label: "emulator suite up", cmd: "bash", args: ["scripts/probe-emulator.sh"], timeoutMs: 120000 },
  ],
  implementAsk: () => `
You are the core area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md fully first. You bootstrap the skeleton every other area builds on — everything you create beyond your implementation is a compiling stub another area will REPLACE WHOLLY later, so keep stubs minimal and never implement their features.
${RULES}
YOUR FILES (you own these; later areas own their own subpaths/crates and will overwrite the stubs):
- Cargo.toml (workspace root): members = crates/nsh-firebase-{core,auth,database,storage,messaging,analytics,perf,firestore,remote-config,crashlytics,installations,app-check,functions} and examples/demo-rs; shared [workspace.dependencies] nsh-core = "0.4.0" and the path deps between nsh-firebase crates; shared profile. Model the style on ~/fo/nsh-query/Cargo.toml (read-only).
- Stub crates: for each of the 12 non-core firebase crates, a minimal compiling crate (Cargo.toml with name + edition + empty [dependencies], lib.rs with a one-line comment). examples/demo-rs: a minimal stub crate too, package name EXACTLY "demo-rs" (the demo area replaces it).
- packages/nsh-firebase: package.json (name nsh-firebase, type module, main/types ./src/index.ts, exports map pre-declaring ".", "./core", "./auth", "./database", "./storage", "./messaging", "./analytics", "./perf", "./monitor", "./firestore", "./remote-config", "./crashlytics", "./installations", "./app-check", "./functions" pointing at src/<area>/index.ts), tsconfig.json, nsh.json (id dev.nsh.firebase, like ~/fo/nsh-query/packages/nsh-query/nsh.json), App.tsx that mounts a small core exercise UI, src/index.ts re-exporting core only, stub src/<area>/index.ts files each containing just "export {};".
- packages/nsh-firebase/src/core/: the real core — initializeApp(FirebaseOptions) + getApp + FirebaseApp; options: projectId "demo-nsh", apiKey/appId/databaseURL http://127.0.0.1:19000, authURL http://127.0.0.1:19099, storageBucket emulator bucket, explicit emulator switches (host/port per product, defaults 127.0.0.1:{19099,19000,19199}); FirebaseError with stable code/message, mapping host-channel failures (ERR_ACCESS_DENIED undeclared, ERR_NSH_HOST_ERROR unhandled/thrown, ERR_FEATURE_UNAVAILABLE_ON_PLATFORM no host); the event bus (on/off/emit, same behavior in TS and Rust); the tier facade: TS — detect the native tier via capabilities.host names "firebase.*" and wrap host.call/host.on; Rust — a small trait (e.g. trait FirebaseHost { async fn call(...) }) with a NativeHost impl over the app's host channel and a PureHost impl used when no host is granted; the pure-tier transports (fetch and WebSocket wrappers over the app's fetch with base URLs from config).
- packages/nsh-firebase/tests/core-app.tsx + tests/core.test.tsx: headless UI tests on a manual clock with stubbed network proving init, config, error mapping (including the criterion-8 test: with the network capability refused/stubbed to deny, a call through the facade rejects with a clear ERR_ACCESS_DENIED), and event bus fan-out. See the test rules above (seen-to-fail-once).
- scripts/ (bash, strict, used by every area and the final smoke): gate-crate.sh <crate-name> → cd repo root; cargo fmt -p <crate> -- --check && cargo clippy -p <crate> --all-targets -- -D warnings && cargo test -p <crate> (deterministic, exit non-zero on any failure). gate-tsx.sh <entry> <testfile> → cd packages/nsh-firebase; nsh check <entry> && out=$(nsh build <entry> 2>&1); print out; fail if it contains a warning (case-insensitive); nsh test <testfile>. gate-workspace.sh → the full Rust gate for all crates (cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test). test-packages.sh → the package unit gates, EXPLICIT FILES: bash scripts/gate-tsx.sh tests/core-app.tsx tests/core.test.tsx; tests/auth-app.tsx tests/auth.test.tsx; tests/database-app.tsx tests/database.test.tsx; tests/storage-app.tsx tests/storage.test.tsx; tests/messaging-app.tsx tests/messaging.test.tsx; tests/analytics-app.tsx tests/analytics.test.tsx; tests/perf-app.tsx tests/perf.test.tsx (each entry is a stub until its area lands — run the gate for a file only if the file is no longer a stub, else skip it and print SKIPPED, exit 0). test-demo.sh → cd examples/demo; nsh check App.tsx; warning-gated nsh build App.tsx; nsh test tests/demo.test.tsx (skip + print SKIPPED if that file does not exist yet); cargo test -p demo-rs. test-emulator.sh → probe the suite with probe-emulator.sh; if down, start it exactly as core does (emulator.pid, log) or wrap with node_modules/.bin/firebase emulators:exec --project demo-nsh --config emulator/firebase.json; then run the labelled sandbox tests, each only if present: cargo test -p nsh-firebase-auth --test emulator_auth -- --ignored; -p nsh-firebase-database --test emulator_database -- --ignored; -p nsh-firebase-storage --test emulator_storage -- --ignored; cd packages/nsh-firebase && nsh test tests/emulator/auth.sandbox.test.tsx; tests/emulator/database.sandbox.test.tsx; tests/emulator/storage.sandbox.test.tsx; tests/emulator/analytics.sandbox.test.tsx and tests/emulator/monitor.sandbox.test.tsx (those two need tools/collector running on 127.0.0.1:18742 — start it detached per docs/status/perfmon.md if down, kill only your own pid after). probe-emulator.sh → curl the auth (19099) and database (19000) ports, exit 0 only if both answer. check-evidence.sh → the evidence manifest (see below). Keep every script plain, deterministic and honest.
- package.json (root): bun package with devDependency firebase-tools, scripts: emulator:start, test:emulator → scripts/test-emulator.sh, test:packages, test:demo, gate:rust → scripts/gate-workspace.sh, gate:ts → scripts/test-packages.sh. Run bun install (firebase-tools must land in node_modules). Append to .gitignore: node_modules/, target/, .nsh/, dist/, emulator logs and pid files, evidence-internal artifacts that must not be committed (keep PNGs and logs committed).
- emulator/: firebase.json (emulators: auth 19099, database 19000, storage 19199, hub 14400, ui disabled; database rules file; storage rules file; storage buckets for demo-nsh), .firebaserc (project demo-nsh), database.rules.json and storage rules — permissive read/write with a 1-2 line comment saying they are sandbox-only for demo-nsh. Then START the suite detached so other areas' sandbox tests can use it: JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home" nohup node_modules/.bin/firebase emulators:start --project demo-nsh --config emulator/firebase.json > emulator/emulator.log 2>&1 & echo $! > emulator/emulator.pid — then wait (probe loop up to ~90s) until 19099 and 19000 answer, and record readiness in docs/status/core.md. If the suite cannot start, report that blocker honestly — sandbox tests elsewhere will report unverified, unit tests still hold.
- docs/status/core.md: what is done, how verified, what is not.
- scripts/check-evidence.sh manifest (the final smoke checks these exist and are non-empty): docs/evidence/native-ios/*.png (>=2), docs/evidence/native-ios/*.log, docs/evidence/native-android/*.png (>=2), docs/evidence/native-android/*.log, docs/evidence/demo/*.png (>=2), docs/evidence/demo/vision.md, docs/evidence/perfmon/*.png (>=2), docs/evidence/perfmon/vision.md, docs/evidence/perfmon/collector.jsonl, docs/evidence/native-ios/simctl-push.log, docs/evidence/native-android/logcat.txt, docs/evidence/analytics/collector-assert.log, docs/evidence/core/access-denied.log. Write the script to fail with a MISSING list.
GATES (your own scoped ones — run them; they must pass): bash scripts/gate-crate.sh nsh-firebase-core; bash scripts/gate-tsx.sh tests/core-app.tsx tests/core.test.tsx; cargo metadata --format-version 1 --no-deps; bash scripts/probe-emulator.sh.
OTHER AREAS (do not touch): auth/database/storage/messaging/analytics/perf+monitor crates and src subpaths; tools/collector (perfmon's); examples/ (demo's and the hosts'); native/ (the native areas'); docs/research.md (research area); fixtures/ (native areas'); docs/README/STATUS (milestone).
Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => {
    coreDone.resolve({ ok: true });
    const probe = await world.run("bash", ["scripts/probe-emulator.sh"], { timeoutMs: 120000 });
    return { criteria: { 8: gateOk(probe) ? "pass" : "unverified" } };
  },
});

// ---------- 2. auth ----------
areas.push({
  name: "auth",
  short: "auth",
  personaName: "z-rust-async-engineer",
  persona: "",
  owns: ["crates/nsh-firebase-auth/", "packages/nsh-firebase/src/auth/", "packages/nsh-firebase/tests/auth-app.tsx", "packages/nsh-firebase/tests/auth.test.tsx", "packages/nsh-firebase/tests/emulator/auth.sandbox.test.tsx", "crates/nsh-firebase-auth/tests/"],
  commitMessage: "feat(auth): Identity Toolkit sign-in with refresh, credentials and currentUser signals",
  auditFocus:
    "Email+password sign-up/sign-in, anonymous, custom token and signInWithCredential taking an ID token, over Identity Toolkit REST per docs/research.md; refresh via Secure Token grant with a manual-clock schedulable refresh; currentUser as a signal in both languages; persistence through a storage trait (Rust) and Storage (TSX); clear FirebaseError mapping; tests fail-first, manual clock, stubbed network for unit tests; the labelled sandbox test signs up, signs in and refreshes against demo-nsh via the emulator suite only through the runner script.",
  gates: [
    { label: "crate auth", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-auth"], timeoutMs: 600000 },
    { label: "ts auth", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/auth-app.tsx", "tests/auth.test.tsx"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the auth area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md, docs/research.md (once the research area lands it) and the core crate/package first. Wait for the core skeleton: your first action after reading is to poll/wait on the workflow's coreDone handoff (it is awaited for you before this ask runs — core's files are stable).
${RULES}
YOUR FILES (replace core's stubs wholly; touch nothing else): crates/nsh-firebase-auth/** and packages/nsh-firebase/src/auth/** plus your test files listed in the ownership line.
TASK (same behavior in Rust and TypeScript, like ~/fo/nsh-query):
- Email+password createUser + signIn, signInAnonymously, signInWithCustomToken, signInWithCredential accepting an ID token from another library — over Identity Toolkit REST (accounts:signUp, accounts:signInWithPassword, accounts:signInAnonymously, accounts:signInWithCustomToken, verifyCustomToken if needed) against the configured base URL (emulator 127.0.0.1:19099 by default).
- Token refresh via Secure Token (grant_type=refresh_token), scheduled so it is testable on a manual clock (next refresh stored as an absolute time computed from Date/clock, not a real timer); expose schedule-check + refresh-now, no hidden real-time waits.
- currentUser as a signal: null | {uid, email?, isAnonymous, idToken, refreshToken, expiresAt...}; authStateReady; signOut.
- Persistence through a storage trait (Rust: a small KeyValueStore trait the app implements; provide an in-memory impl; TSX: nsh's Storage) — restore on init.
- Errors: map Identity Toolkit error codes to FirebaseError with clear messages; never log or store credentials beyond what the flow needs.
- Unit tests (fail-first, manual clock, stubbed network): sign-up, sign-in, anonymous, custom token, credential sign-in, refresh flow with the clock moved forward, persistence restore, error paths (bad email, wrong password, network refused).
- Labelled sandbox test (only ever run by scripts/test-emulator.sh; Rust: crates/nsh-firebase-auth/tests/emulator_auth.rs with #[ignore = "sandbox: needs emulator suite"]; TSX: packages/nsh-firebase/tests/emulator/auth.sandbox.test.tsx, network live to 127.0.0.1 only, declared capabilities): signs up demo@demo-nsh.test / a fake password, signs in, refreshes, reads back currentUser — against demo-nsh on the emulator suite.
GATES: bash scripts/gate-crate.sh nsh-firebase-auth; bash scripts/gate-tsx.sh tests/auth-app.tsx tests/auth.test.tsx.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => ({}),
});

// ---------- 3. database ----------
areas.push({
  name: "database",
  short: "database",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: ["crates/nsh-firebase-database/", "packages/nsh-firebase/src/database/", "packages/nsh-firebase/tests/database-app.tsx", "packages/nsh-firebase/tests/database.test.tsx", "packages/nsh-firebase/tests/emulator/database.sandbox.test.tsx", "crates/nsh-firebase-database/tests/"],
  commitMessage: "feat(database): Realtime Database REST with SSE live listeners, queries and transactions",
  auditFocus:
    "refs, get, set, update, remove, push, transactions and queries over the RTDB REST API; live listeners over Server-Sent Events with explicit reconnect and offline behavior documented; listener events as signals; tests fail-first on a manual clock with stubbed network; the labelled sandbox test writes, reads, and receives a live update through a listener against demo-nsh.",
  gates: [
    { label: "crate database", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-database"], timeoutMs: 600000 },
    { label: "ts database", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/database-app.tsx", "tests/database.test.tsx"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the database area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md and docs/research.md first; the core skeleton is stable (coreDone is awaited before this ask runs).
${RULES}
YOUR FILES (replace core's stubs wholly; touch nothing else): crates/nsh-firebase-database/** and packages/nsh-firebase/src/database/** plus your test files.
TASK (same behavior in Rust and TS):
- ref(path), child, get, set, update (multi-path), remove, push (client-generated push ids with the same ordering properties — verify the algorithm from the official REST docs or reference sources, cite in a comment), transactions (REST conditional get/put with etag/if-match semantics per docs/research.md), queries (orderByChild/key/value/limitToFirst/Last, startAt/endAt/equalTo) encoded as REST query params.
- Live listeners over SSE: onValue/onDisconnect semantics kept honest — document exactly what happens on network loss (reconnect with backoff on a manual clock, resync from the last state) in the crate docs and docs/status/database.md.
- Listener data flows into signals so UI bindings update (TSX signal / Rust signal).
- Unit tests (fail-first, manual clock, stubbed network incl. an SSE stub route the clock can advance): CRUD, push ordering, transaction retry, query params, listener receives then reconnects.
- Labelled sandbox test (runner-only; same file conventions as auth): writes a value, reads it back, subscribes a listener, writes from a second client path and receives the live update — against demo-nsh on 127.0.0.1:19000.
GATES: bash scripts/gate-crate.sh nsh-firebase-database; bash scripts/gate-tsx.sh tests/database-app.tsx tests/database.test.tsx.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => ({}),
});

// ---------- 4. storage ----------
areas.push({
  name: "storage",
  short: "storage",
  personaName: "z-rust-async-engineer",
  persona: "",
  owns: ["crates/nsh-firebase-storage/", "packages/nsh-firebase/src/storage/", "packages/nsh-firebase/tests/storage-app.tsx", "packages/nsh-firebase/tests/storage.test.tsx", "packages/nsh-firebase/tests/emulator/storage.sandbox.test.tsx", "crates/nsh-firebase-storage/tests/"],
  commitMessage: "feat(storage): resumable uploads with progress, download URLs, metadata, list and delete",
  auditFocus:
    "Resumable upload over the Cloud Storage REST paths per docs/research.md with progress events and true resume from an upload id; download URL, metadata get/update, list, delete; progress as a signal; tests fail-first on a manual clock with chunked stub routes; the labelled sandbox test uploads with progress, downloads and deletes against demo-nsh.",
  gates: [
    { label: "crate storage", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-storage"], timeoutMs: 600000 },
    { label: "ts storage", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/storage-app.tsx", "tests/storage.test.tsx"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the storage area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md and docs/research.md first; core is stable (coreDone awaited).
${RULES}
YOUR FILES (replace core's stubs wholly; touch nothing else): crates/nsh-firebase-storage/** and packages/nsh-firebase/src/storage/** plus your test files.
TASK (same behavior in Rust and TS):
- ref(path) → upload(bytes, metadata?) as a resumable upload per docs/research.md (X-Upload-Content-Length, chunked PUTs to the upload session URL, resume from a saved upload_id); progress as a signal (bytesTransferred, totalBytes, state paused|running|done|error); pause()/resume()/cancel() driven by the caller, all clock-testable.
- downloadURL, getMetadata/updateMetadata, list(prefix, maxResults, pageToken), delete.
- Unit tests (fail-first, manual clock, stubbed chunked routes incl. an interrupted-then-resumed upload): progress sequence, resume from a mid-upload id, metadata round-trip, list paging, delete, error paths.
- Labelled sandbox test (runner-only): upload a small fixture file with progress events, get its download URL, download and compare bytes, delete — against demo-nsh storage on 127.0.0.1:19199.
GATES: bash scripts/gate-crate.sh nsh-firebase-storage; bash scripts/gate-tsx.sh tests/storage-app.tsx tests/storage.test.tsx.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => ({}),
});

// ---------- 5. messaging ----------
areas.push({
  name: "messaging",
  short: "messaging",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: ["crates/nsh-firebase-messaging/", "packages/nsh-firebase/src/messaging/", "packages/nsh-firebase/tests/messaging-app.tsx", "packages/nsh-firebase/tests/messaging.test.tsx", "crates/nsh-firebase-messaging/tests/"],
  commitMessage: "feat(messaging): one facade over the native tier with a clearly-failing pure stub",
  auditFocus:
    "One facade, same names in TS and Rust, matching the unreleased nsh Notifications names (permission, requestPermission, show({id,title,body,silent}), close(id), setBadge, on(\"press\")) plus token/on(\"token\")/subscribe/unsubscribe and on(\"message\"); pure tier rejects with a CLEAR error naming the native tier requirement (ERR_FEATURE_UNAVAILABLE_ON_PLATFORM or a documented library code — no silent no-ops); host handler names exactly firebase.messaging.* per the contract table; native areas implement the same table, so any drift is a finding.",
  gates: [
    { label: "crate messaging", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-messaging"], timeoutMs: 600000 },
    { label: "ts messaging", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/messaging-app.tsx", "tests/messaging.test.tsx"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the messaging area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md, docs/research.md, and ~/fo/.refs/nsh-main's Notifications docs+source (crates/nsh-core/src/notifications.rs, website/modules/nsh.md section Notifications) first. researchDone and coreDone are awaited before this ask runs.
${RULES}
YOUR FILES (replace core's stubs wholly; touch nothing else): crates/nsh-firebase-messaging/** and packages/nsh-firebase/src/messaging/** plus your test files. The NATIVE tier handlers themselves belong to the native-ios and native-android areas — you own the facade contract they implement.
TASK (facade identical in Rust and TS):
- THE CONTRACT (put it verbatim in your crate docs and docs/status/messaging.md so the native areas match it): facade API messaging.permission() / messaging.requestPermission() → "default"|"granted"|"denied"|"unavailable" (same strings as nsh Notifications); messaging.show({id,title,body,silent?}) → Promise<boolean>; messaging.close(id); messaging.setBadge(n); messaging.on("press", cb) with {id}; messaging.on("message", cb) with the FCM message payload; messaging.on("token", cb) with {token}; messaging.token() → Promise<string> (rejects clearly when unavailable or the token request fails); messaging.subscribe(topic)/unsubscribe(topic). Host handlers the native tier registers (declare "firebase.messaging.*" in capabilities.host): firebase.messaging.token, .permission, .requestPermission, .show, .close, .setBadge, .subscribe, .unsubscribe (calls); host→app events: firebase.messaging.message, firebase.messaging.tokenRefreshed, firebase.messaging.press.
- Native tier available: the facade transparently routes those calls/events through the tier facade from core.
- Pure tier (no host handlers granted, or web/desktop/nsh-package builds): every messaging call rejects with a CLEAR error that names what is missing ("Firebase Messaging needs the native tier: iOS/Android host with firebase.messaging.* handlers") using ERR_FEATURE_UNAVAILABLE_ON_PLATFORM or a documented FirebaseError code — no silent no-ops, no fake tokens.
- Unit tests (fail-first, manual clock, host stubs via the test host handler feature): happy path through a stubbed native host (token, show, press event delivery, message delivery), pure-tier clear failures for every call, subscription/unsubscription pass-through, badge pass-through.
GATES: bash scripts/gate-crate.sh nsh-firebase-messaging; bash scripts/gate-tsx.sh tests/messaging-app.tsx tests/messaging.test.tsx.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => ({}),
});

// ---------- 6. analytics ----------
areas.push({
  name: "analytics",
  short: "analytics",
  personaName: "z-rust-async-engineer",
  persona: "",
  owns: ["crates/nsh-firebase-analytics/", "packages/nsh-firebase/src/analytics/", "packages/nsh-firebase/tests/analytics-app.tsx", "packages/nsh-firebase/tests/analytics.test.tsx", "packages/nsh-firebase/tests/emulator/analytics.sandbox.test.tsx", "crates/nsh-firebase-analytics/tests/"],
  commitMessage: "feat(analytics): batching event queue with pluggable transports and consent",
  auditFocus:
    "logEvent, setUserProperty, setUserId, screen views, setConsent, setDebugViewEnabled; a batching queue (flush interval + max batch size) fully deterministic on the manual clock; pluggable transport with a GA4 Measurement Protocol transport whose limits are documented and a collector transport for sandbox runs; native-tier routing through the facade; tests fail-first with deterministic flush assertions.",
  gates: [
    { label: "crate analytics", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-analytics"], timeoutMs: 600000 },
    { label: "ts analytics", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/analytics-app.tsx", "tests/analytics.test.tsx"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the analytics area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md and docs/research.md first; core is stable (coreDone awaited).
${RULES}
YOUR FILES (replace core's stubs wholly; touch nothing else): crates/nsh-firebase-analytics/** and packages/nsh-firebase/src/analytics/** plus your test files. tools/collector belongs to the perfmon area — for your own sandbox test, point the collector transport at its fixed port (http://127.0.0.1:18742/collect) but do NOT create or start it; your TSX sandbox test must skip honestly (report unverified) if the collector port is closed. Better: your unit tests use an in-crate in-memory transport; the sandbox test uses the collector transport.
TASK (same behavior in Rust and TS):
- analytics(app): logEvent(name, params?), setUserProperty(key, value), setUserId, logScreenView({screenName, screenClass?}), setConsent({analyticsStorage: "granted"|"denied", ...}) — consent DENIED must actually stop transport sends, setDebugViewEnabled(bool) (pure tier: a documented no-op flag; native tier routes to the SDK through the facade).
- Batching queue: configurable flushIntervalMs and maxBatchSize; sends are driven by a clock-checkable scheduler (manual-clock deterministic, no real timers in unit tests); events validated (name/param rules from the GA4 docs summary in docs/research.md); retries with backoff on transport failure (clock-driven).
- Pluggable transport: trait/interface Transport { send(batch) }; GA4 Measurement Protocol transport (POST to the MP endpoint with api_secret + measurement_id from config; client_id from Installations-like stable random id stored via the storage trait; document MP's honest limits inline and in docs/status/analytics.md — no automatic sessions, no retargeting audiences, etc.); collector transport posting JSON lines to /collect; native transport routing events through the tier facade when firebase.analytics.* handlers exist (the native areas register them).
- Unit tests (fail-first, manual clock, in-memory transport): event batching at maxBatchSize, flush on interval, consent blocks sends, retry then success, screen views shape, user property merge.
- Labelled sandbox test (runner-only; TSX: tests/emulator/analytics.sandbox.test.tsx): sends three events to the collector at 127.0.0.1:18742/collect and asserts via reading back the collector's own assertion output (see perfmon's collector contract: GET /assert returns what was received) — if the collector is down, report unverified honestly.
GATES: bash scripts/gate-crate.sh nsh-firebase-analytics; bash scripts/gate-tsx.sh tests/analytics-app.tsx tests/analytics.test.tsx.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async () => ({}),
});

// ---------- 7. perfmon (performance + network monitoring) ----------
areas.push({
  name: "perfmon",
  short: "perfmon",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: ["crates/nsh-firebase-perf/", "packages/nsh-firebase/src/perf/", "packages/nsh-firebase/src/monitor/", "packages/nsh-firebase/tests/perf-app.tsx", "packages/nsh-firebase/tests/perf.test.tsx", "packages/nsh-firebase/tests/emulator/monitor.sandbox.test.tsx", "crates/nsh-firebase-perf/tests/", "tools/collector/", "examples/demo/src/screens/monitor.tsx and examples/demo/tests/perfmon-shots.test.tsx (only these two files inside examples/demo)"],
  commitMessage: "feat(perf): traces, HTTP metrics, the network monitor, collector sink and the monitoring screen",
  auditFocus:
    "The exact NetworkEvent interface from PROMPT.md (id, method, url, host, path, status, startedAtMs, durationMs, ttfbMs, requestBytes, responseBytes, error, initiator); instrumented fetch wrapper in TS and Rust; monitor with ring buffer and per-host/per-path aggregates (count, error rate, p50, p95); perf traces (startTrace/putMetric/putAttribute/stop) and HTTP metrics built from NetworkEvents into Performance Monitoring; tools/collector sink with /collect, /slow (configurable delay) and /assert; the demo monitoring screen file (sorted by duration, filterable, slow route visibly on top); native automatic capture documented honestly.",
  gates: [
    { label: "crate perf", cmd: "bash", args: ["scripts/gate-crate.sh", "nsh-firebase-perf"], timeoutMs: 600000 },
    { label: "ts perf+monitor", cmd: "bash", args: ["scripts/gate-tsx.sh", "tests/perf-app.tsx", "tests/perf.test.tsx"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the perfmon area (performance + network monitoring) of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md's Network monitoring section verbatim and docs/research.md first; core is stable (coreDone awaited). Your screen file waits for the demo shell (demoShellReady is awaited for you mid-task — build everything else first, then write the screen).
${RULES}
YOUR FILES (replace core's stubs wholly; touch nothing else): crates/nsh-firebase-perf/**, packages/nsh-firebase/src/perf/**, packages/nsh-firebase/src/monitor/**, your test files, tools/collector/**, and EXACTLY ONE file inside the demo app: examples/demo/src/screens/monitor.tsx (the demo area owns everything else there — write your file against the contract below once the shell exists).
TASK (same behavior in Rust and TS):
- The EXACT NetworkEvent interface from PROMPT.md as the shared type in src/monitor (TS) and the crate (Rust, serde-serializable with identical JSON): {id, method, url, host, path, status: number|null, startedAtMs, durationMs, ttfbMs: number|null, requestBytes: number|null, responseBytes: number|null, error: string|null, initiator: string|null}.
- instrumentFetch: a drop-in fetch wrapper emitting NetworkEvents (id via crypto, timing via performance.now on the manual clock, host/path parsed from the URL, ttfb from response headers-ready, byte sizes from content-length or body length when known, initiator "firebase-fetch"); Rust: an instrumented wrapper over the core transport. Both must compose with nsh-query (fetch-compatible).
- monitor: ring buffer (default 500 events, configurable), per-host and per-path aggregates {count, errorRate, p50, p95} computed deterministically, subscribe API; a sorted/filterable view model for a screen (sortKey duration desc; filters host/path/minDuration/status).
- perf: startTrace(name) → {putMetric, putAttribute, stop} recording durations on the clock; recordHttpMetric(event: NetworkEvent) building HTTP metrics from monitor events; traces/metrics go out through a pluggable transport (same shape as analytics') and route through the native tier when firebase.perf.* handlers exist; document honestly what the native automatic capture adds and what stays invisible (docs/status/perfmon.md).
- tools/collector: a small local HTTP sink (bun single-file server, own port 18742): POST /collect appends each JSON body as a line to docs/evidence/perfmon/collector.jsonl (and --out to override); GET /assert?contains=<substr>&count=<n> returns {"ok":bool,"count":n,"matches":k} for tests to assert what was sent; POST|GET /slow?ms=1500 replies after ms (default 1500) — this is the demo's deliberately slow route; GET /health → 200. Startable via scripts or bun run; document its one-command start in its README section of docs/status/perfmon.md. Unit-test the sink logic (no server needed for the logic tests).
- Unit tests (fail-first, manual clock, stubbed routes): event capture fields, ring buffer eviction, p50/p95 math, aggregates per host/path, trace metric/attribute/stop durations, httpMetric from events, batched transport flush, screen view-model sorting/filters.
NOTE: the demo monitoring screen file is written in a second pass (the workflow runs it once the demo shell exists) — do not block on it now.
GATES: bash scripts/gate-crate.sh nsh-firebase-perf; bash scripts/gate-tsx.sh tests/perf-app.tsx tests/perf.test.tsx.
Reply JSON only: ${REPORT_FORMAT}`,
  mid: async (a, impl) => {
    await raceTimeout(demoShellReady.promise, 75 * 60 * 1000, "demo shell").catch(() => {});
    const screen = asJson(await ask(impl,
      "SECOND PASS — the demo shell now exists. Write your two demo-owned files:\n" +
      "1) examples/demo/src/screens/monitor.tsx — replace the demo's one-line placeholder wholly with the real monitoring screen: named export MonitorScreen (no props), reading the shared monitor instance from nsh-firebase/monitor, theme tokens only (nsh-component-library rules), a sorted (duration desc) filterable list of live calls fed by a live collector /slow route (~1200ms) plus two fast routes on 127.0.0.1:18742, per-call method/path/duration/ttfb/status, slowest visibly on top. It must fit the demo's navigation contract (route \"monitor\").\n" +
      "2) examples/demo/tests/perfmon-shots.test.tsx — a real-platform screenshot test that drives the monitor route with the collector's responses and calls app.screenshot() to docs/evidence paths given to you later; network stubs or live loopback only (no real internet).\n" +
      "Then re-run your scoped gates: bash scripts/gate-crate.sh nsh-firebase-perf; bash scripts/gate-tsx.sh tests/perf-app.tsx tests/perf.test.tsx; plus cd examples/demo && nsh check App.tsx (so your files compile inside the demo). Update docs/status/perfmon.md.\n" +
      "Reply JSON only: " + REPORT_FORMAT));
    return screen.done === true ? "screen landed" : "SCREEN FAILED: " + String(screen.summary ?? "") + " | " + JSON.stringify(screen.blockers ?? []);
  },
  run: async (a, impl) => {
    // criterion 7 evidence: macos screenshot + phone screenshot + a vision judge
    const shots = asJson(await ask(impl,
      "VISION+DEVICE EVIDENCE for criterion 7 (the monitoring screen): 1) Start tools/collector with a /slow route (~1200ms) on 127.0.0.1:18742 (detached, log to docs/evidence/perfmon/collector.log, pid to docs/evidence/perfmon/collector.pid — it is your own process). 2) Add a real-platform screenshot test (tests/screenshots.macos.test.tsx, your file under packages/nsh-firebase/tests or the demo's tests dir per demo's nsh.json — ask for nothing from the demo area; if the screen lives under examples/demo, run its screenshot from examples/demo with your own new test file named tests/perfmon-shots.test.tsx and take app.screenshot() after driving the monitor route with fetched timings from /slow and two fast routes). Run it with nsh test --target macos so pixels are real; save PNGs to docs/evidence/perfmon/monitor-macos-1.png (+2 if useful) and the command log to docs/evidence/perfmon/macos.log. 3) Save the collector.jsonl copy showing the three requests with their durations to docs/evidence/perfmon/collector.jsonl. 4) Report the PNG paths. Reply JSON only: " + REPORT_FORMAT));
    const phone = await askIOS(
      "DEVICE TASK (perfmon, criterion 7): the repo's iPhone simulator nsh-firebase-ios (create it if missing: xcrun simctl create nsh-firebase-ios \"iPhone 17\" — iOS 27 runtime; boot it; it is yours alone, never touch other devices). Then: run the monitoring-screen screenshot test on the simulator (cd /Users/hmziq/fo/nsh-firebase/packages/nsh-firebase or examples/demo as perfmon's evidence report states; nsh test --target ios tests/perfmon-shots.test.tsx with the collector already running), capture via app.screenshot to /Users/hmziq/fo/nsh-firebase/docs/evidence/perfmon/monitor-ios-1.png, and save the run log to docs/evidence/perfmon/ios.log. If nsh cannot target the simulator, fall back to running the demo host app and xcrun simctl io <udid> screenshot — still your own device. Report PNG + log paths. Reply JSON only: {\"done\":bool,\"summary\":\"...\",\"blockers\":[...],\"evidence\":\"...\"}"
    ).catch((e: any) => ({ done: false, blockers: [String(e)] }));
    const judge = agent("QA evaluator·perfmon-vision", { system: (await specialist("z-qa-evaluator", "You judge screenshots honestly.")).persona, model: MODELS.flash });
    const vision = asJson(await ask(judge,
      "VISION JUDGE for criterion 7. Use the Read tool to LOOK at every PNG in /Users/hmziq/fo/nsh-firebase/docs/evidence/perfmon/ (monitor-macos-1.png, monitor-ios-1.png and any others) — look at each file, do not trust filenames. PASS only if: the monitoring screen is visible with a sorted call list, per-call timings are shown, and the deliberately slow route (~1200ms) is visibly at the TOP of the list; filters visible. Record what you actually see per PNG. Write your verdict to /Users/hmziq/fo/nsh-firebase/docs/evidence/perfmon/vision.md (file paths, what each PNG shows, pass/fail per image, overall verdict, anything unverifiable). Reply JSON only: {\"done\":bool,\"pass\":<bool>,\"looked\":[{\"png\":\"...\",\"what I see\":\"...\"}],\"issues\":[...],\"summary\":\"...\"}"));
    const collectorOk = await runSpecs([{ label: "collector log has the slow request", cmd: "bash", args: ["-c", "test -s docs/evidence/perfmon/collector.jsonl && grep -q '/slow' docs/evidence/perfmon/collector.jsonl && bash scripts/check-evidence.sh"] , timeoutMs: 120000 }]);
    return {
      criteria: { 7: vision.pass === true && collectorOk.ok ? "pass" : "unverified" },
      extra: { phone: phone.summary ?? "phone capture failed", vision: vision.summary ?? "" },
    };
  },
});

// ---------- 8. native-ios ----------
areas.push({
  name: "native-ios",
  short: "native-ios",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: ["native/ios/NshFirebase/", "examples/host-ios/", "fixtures/GoogleService-Info.plist", "docs/setup-ios.md"],
  commitMessage: "feat(native-ios): SwiftPM Firebase package, NshAppView handlers and the host-ios example",
  auditFocus:
    "native/ios/NshFirebase is a Swift package over the Firebase Apple SDK (SPM) that configures FirebaseApp from the FAKE plist and registers exactly the firebase.messaging.* / firebase.analytics.* / firebase.perf.* handlers from the messaging contract on NshAppView, posts firebase.messaging.{message,tokenRefreshed,press} back to the app, mirrors RNFB's delegate takeover patterns (cited from ~/fo/.refs/react-native-firebase by path), shows received FCM messages via UNUserNotificationCenter matching nsh's Notifications semantics; examples/host-ios is a real xcodegen-generated Xcode project modelled on ~/fo/.refs/nsh-v0.4.0/examples/embed-ios that embeds the demo bundle and the package; fixtures/GoogleService-Info.plist has obviously fake values; docs/setup-ios.md covers what the RNFB Expo plugin automates (config files, delegates, entitlements) for this repo by hand; no real credentials anywhere.",
  gates: [
    { label: "swift package builds", cmd: "bash", args: ["-c", "cd native/ios/NshFirebase && swift build"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the native-ios area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md first; researchDone is awaited before this ask (docs/research.md has your per-file citations — follow them). The demo bundle may not exist yet; your BUILD-time work needs nothing from it (the device phase uses the demo bundle when it exists).
${RULES}
YOUR FILES: native/ios/NshFirebase/**, examples/host-ios/**, fixtures/GoogleService-Info.plist, docs/setup-ios.md, plus docs/status/native-ios.md.
TASK:
1. fixtures/GoogleService-Info.plist with obviously fake values (PROJECT_ID demo-nsh, API_KEY "fake-ios-key-abc123", GOOGLE_APP_ID "1:123456789012:ios:abc123def456", BUNDLE_ID matching the host app, GCM_SENDER_ID "123456789012", STORAGE_BUCKET "demo-nsh.appspot.com"). A comment line in docs/setup-ios.md states it is fake.
2. native/ios/NshFirebase: SwiftPM Package.swift depending on the Firebase Apple SDK via SPM (pin a version, record which in docs/status/native-ios.md); sources: NshFirebase.swift — configure(from: plist) using FIROptions with the fake values, then registerHandlers(on: NshAppView) installing EXACTLY the contract handlers from docs/status/messaging.md (read it if present; otherwise the table in your contract): firebase.messaging.token/.permission/.requestPermission/.show/.close/.setBadge/.subscribe/.unsubscribe + firebase.analytics.logEvent/.setUserId/.setUserProperty/.logScreenView/.setConsent + firebase.perf.startTrace/.stopTrace/.recordHttpMetric; host→app events via app.send: firebase.messaging.message, firebase.messaging.tokenRefreshed, firebase.messaging.press. Mirror RNFB's delegate takeover for MessagingDelegate + UNUserNotificationCenter (forward to previous delegates), show received messages through UNUserNotificationCenter with nsh Notifications semantics ({id,title,body,silent}), and keep presses queueable until the app listens (like nsh-main's module). Analytics: FIRAnalytics.log etc.; Performance: automatic traces ON + FIRHttpMetric-style capture where the SDK provides it. Model every delegate pattern on the cited RNFB files; NEVER invent Firebase API names — verify in the Firebase Apple SDK sources resolved by SPM or the RNFB reference.
3. examples/host-ios: a real Xcode project GENERATED WITH xcodegen (installed; write project.yml) modelled on ~/fo/.refs/nsh-v0.4.0/examples/embed-ios/main.swift: NshAppView, grantCapabilities including "network" for 127.0.0.1 and "host": ["firebase.*"], loadBundle from nsh/bundle.nshc, NshFirebase attached before load, a visible status label of what handlers fired. Build target: iOS Simulator. The demo bundle path to embed is examples/demo/dist/nsh (the demo area builds it); ALSO build your own minimal fallback TSX bundle (examples/host-ios/mini-app/App.tsx + nsh build) so the host is testable before the demo lands.
4. Build headlessly: xcodebuild -project examples/host-ios/*.xcodeproj -scheme <scheme> -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' -derivedDataPath examples/host-ios/build build — save the log tail to docs/evidence/native-ios/xcodebuild.log. SPM will fetch the Firebase SDK (big; be patient; never cancel the machine's other builds).
5. docs/setup-ios.md: what RNFB's Expo plugin automates (from ~/fo/.refs/react-native-firebase/packages/app/plugin/src/ios) and the manual equivalent here (plist, package link, delegate wiring, push entitlement note), direct and concrete.
GATES: swift build of the package (above) must pass. Do NOT run the simulator yourself — the workflow's device verifier does that after your gates.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async (a, impl) => {
    await raceTimeout(demoBundleReady.promise, 90 * 60 * 1000, "demo bundle").catch(() => {});
    const dev = asJson(await askIOS(
      "DEVICE TASK (native-ios, criterion 4) — all under /Users/hmziq/fo/nsh-firebase, your device only:\n" +
      "1) Ensure simulator nsh-firebase-ios exists (xcrun simctl create nsh-firebase-ios \"iPhone 17\") and is booted (by UDID; it may already be booted from an earlier task).\n" +
      "2) Determine the app bundle to install: the native-ios area's report names the built host app under examples/host-ios/build/Build/Products/Debug-iphonesimulator/ (install the .app). If the demo bundle examples/demo/dist/nsh exists, ensure it is the embedded bundle (the area embeds it during xcodebuild via its nsh folder; if the app was built with the mini fallback, say so honestly in the report).\n" +
      "3) xcrun simctl install <udid> <app>; xcrun simctl launch <udid> <bundle-id>; wait ~15s for boot+load; xcrun simctl io <udid> screenshot docs/evidence/native-ios/host-sim-1.png (the demo should be visible inside the host).\n" +
      "4) Criterion 4 messaging proof WITHOUT a real token: start a unified-log capture (xcrun simctl spawn <udid> log stream --style compact > docs/evidence/native-ios/unified.log & — remember its pid, kill only that pid at the end). Then xcrun simctl push <udid> <bundle-id> with an APNs payload file (write docs/evidence/native-ios/push.apns with aps alert {title \"FCM proof\", body \"tap me\"} and a message payload the host logs). Capture docs/evidence/native-ios/simctl-push.log (the push command output + 10s of unified log). Take host-sim-2.png showing the notification/banner or the app's rendered message. Then TAP the banner with idb (idb ui tap x y on the booted sim; find the banner coordinates from host-sim-2.png) and capture host-sim-3.png + unified.log tail showing the app's firebase.messaging.press / opened handler fired. If tap automation genuinely fails, report exactly what was and was not proven — never fake it.\n" +
      "5) Save every command + output into docs/evidence/native-ios/simctl-push.log. Reply JSON only: {\"done\":bool,\"summary\":\"...\",\"blockers\":[...],\"evidence\":\"docs/evidence/native-ios/...\"}"
    ).catch((e: any) => ({ done: false, blockers: [String(e)] })));
    const judge = agent("QA evaluator·native-ios-vision", { system: (await specialist("z-qa-evaluator", "You judge screenshots honestly.")).persona, model: MODELS.flash });
    const vision = asJson(await ask(judge,
      "VISION JUDGE for criterion 4. Read tool: LOOK at every PNG under /Users/hmziq/fo/nsh-firebase/docs/evidence/native-ios/. PASS only if: the host app shows the embedded demo content (not a blank screen), a notification/banner or in-app FCM message is visible in at least one PNG, and a post-tap PNG exists showing the app handled the tap (or the judge records honestly that tap evidence is missing). Write /Users/hmziq/fo/nsh-firebase/docs/evidence/native-ios/vision.md with what each PNG shows and the verdict. Reply JSON only: {\"done\":bool,\"pass\":bool,\"looked\":[...],\"issues\":[...],\"summary\":\"...\"}"));
    return {
      criteria: { 4: dev.done === true && vision.pass === true ? "pass" : "unverified" },
      extra: { device: dev.summary ?? "", vision: vision.summary ?? "" },
    };
  },
});

// ---------- 9. native-android ----------
areas.push({
  name: "native-android",
  short: "native-android",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: ["native/android/", "examples/host-android/", "fixtures/google-services.json", "docs/setup-android.md"],
  commitMessage: "feat(native-android): Gradle module on the Firebase BoM, NshView handlers and the host-android example",
  auditFocus:
    "native/android is a Gradle module on the Firebase BoM (version pinned, recorded) exposing an NshFirebase object that initializes FirebaseApp from the FAKE google-services.json values and registers exactly the firebase.messaging.* / firebase.analytics.* / firebase.perf.* handlers on NshView per the messaging contract, shows messages via NotificationManager with nsh Notifications semantics, handles intent passthrough (handleNotificationIntent) and background delivery modelled on RNFB's ReactNativeFirebaseMessagingService/HeadlessService (cited by path); examples/host-android is a real Gradle app (xcodegen's Android counterpart: plain Gradle files) modelled on ~/fo/.refs/nsh-v0.4.0/examples/embed-android embedding nsh.aar + the demo bundle; fixtures/google-services.json obviously fake; docs/setup-android.md covers the plugin-automated steps manually; no real credentials.",
  gates: [
    { label: "gradle module + app assemble", cmd: "bash", args: ["-c", "export JAVA_HOME=\"/Applications/Android Studio.app/Contents/jbr/Contents/Home\" && cd examples/host-android && ./gradlew :app:assembleDebug --console=plain -q"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the native-android area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md first; researchDone is awaited (docs/research.md cites your per-file patterns — follow them). Always export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home" for Gradle. The demo bundle may not exist yet; build-time work needs nothing from it.
${RULES}
YOUR FILES: native/android/**, examples/host-android/**, fixtures/google-services.json, docs/setup-android.md, plus docs/status/native-android.md.
TASK:
1. fixtures/google-services.json with obviously fake values (project_id demo-nsh, project_number 123456789012, api_key fake-android-key-abc123, mobilesdk_app_id 1:123456789012:android:abc123def456, package_name matching the host app). Marked fake in docs/setup-android.md.
2. native/android: a Gradle library module (Gradle files, Kotlin or Java — match what nsh.aar's embed example uses: Java) on the Firebase BoM (com.google.firebase:firebase-bom, pin the version and record it) with firebase-messaging, firebase-analytics, firebase-perf; NshFirebase.java: init(Context) from the fake values (FirebaseApp.initializeApp with FirebaseOptions or google-services plugin applied to the HOST app only — choose the approach that needs no real credentials and document it), register(NshView) installing EXACTLY the contract handlers (firebase.messaging.token/.permission/.requestPermission/.show/.close/.setBadge/.subscribe/.unsubscribe, firebase.analytics.*, firebase.perf.*) and nsh.send(...) for firebase.messaging.{message,tokenRefreshed,press}; FirebaseMessagingService subclass + manifest entries modelled on RNFB's ReactNativeFirebaseMessagingService/HeadlessService (cite paths); show received messages via NotificationManager with nsh Notifications semantics; NshView.handleNotificationIntent wired per nsh-main's docs (onCreate/onNewIntent); token requested with the fake project — its FAILURE must surface as a clear error through the facade (that is the criterion-5 proof, not a real token).
3. examples/host-android: a real Gradle app (settings.gradle, build.gradle, app/, AndroidManifest) modelled on ~/fo/.refs/nsh-v0.4.0/examples/embed-android/src/com/longbridge/nsh/embeddemo/MainActivity.java: NshView, capabilities granted incl. network 127.0.0.1 + host firebase.*, bundle from assets/nsh (nsh build --release --out into src/main/assets/nsh), NshFirebase.init + register before load, a status TextView of what handlers fired. Embed nsh.aar and your module; the demo bundle goes in when examples/demo/dist/nsh exists (device phase) — also keep a mini fallback bundle (examples/host-android/mini-app) so the host is testable before the demo lands.
4. Build headlessly (wrapper-less projects: use the Android Studio Gradle; if no wrapper exists, invoke gradle from $ANDROID_HOME or Android Studio's distribution — record exactly which gradle binary you used in docs/status/native-android.md): JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home" gradle/gradlew :app:assembleDebug, log tail to docs/evidence/native-android/gradle.log. Google Maven fetches the BoM (big; be patient).
5. docs/setup-android.md: what RNFB's Expo plugin automates for Android (from packages/app/plugin/src/android) and the manual equivalent here (google-services.json, BoM, service manifest entries, notification channel, POST_NOTIFICATIONS), direct and concrete.
GATES: the Gradle assembleDebug above must pass. Do NOT run the emulator yourself — the device verifier does that.
Reply JSON only: ${REPORT_FORMAT}`,
  run: async (a, impl) => {
    await raceTimeout(demoBundleReady.promise, 90 * 60 * 1000, "demo bundle").catch(() => {});
    const dev = asJson(await askAndroid(
      "DEVICE TASK (native-android, criterion 5) — all under /Users/hmziq/fo/nsh-firebase, your emulator only:\n" +
      "1) Ensure the repo's own AVD exists and boots: if `avdmanager list avd` (with JAVA_HOME=/Applications/Android Studio.app/Contents/jbr/Contents/Home) lacks nsh-firebase, create it: avdmanager create avd -n nsh-firebase -k \"system-images;android-37.0;google_apis_playstore_ps16k;arm64-v8a\" -d pixel_7 (verify the system image dir exists first; pick the newest google_apis_playstore arm64 image present). Boot detached on YOUR port: nohup $ANDROID_HOME/emulator/emulator -avd nsh-firebase -port 15557 -no-snapshot-save -no-boot-anim > /Users/hmziq/fo/nsh-firebase/docs/evidence/native-android/emulator.log 2>&1 & echo $! > docs/evidence/native-android/emulator.pid; wait for adb -s emulator-15557 shell getprop sys.boot_completed = 1. Use adb -s emulator-15557 on EVERY call. Never touch other emulators/AVDs.\n" +
      "2) Install the host APK the native-android area built (examples/host-android/app/build/outputs/apk/debug/app-debug.apk) and launch its activity. If the demo bundle examples/demo/dist/nsh exists it is already the embedded bundle; if the mini fallback is embedded, say so honestly.\n" +
      "3) adb -s emulator-15557 exec-out screencap -p > docs/evidence/native-android/host-emulator-1.png (demo visible inside the host).\n" +
      "4) Messaging proof WITHOUT a real token: start logcat capture (adb -s emulator-15557 logcat -v time > docs/evidence/native-android/logcat.txt & — remember the pid, kill only it at the end). Foreground: drive the message path (adb shell am broadcast to the app's receiver, or trigger the host's own 'send' UI, or invoke the service per the area's docs — the area documents the exact command in docs/status/native-android.md; use it) and capture host-emulator-2.png showing the rendered message/notification. Background: send the same while the activity is stopped (adb shell input keyevent KEYCODE_HOME first), prove delivery in logcat, then bring the app back and capture host-emulator-3.png. Tap path: open the notification via adb shell cmd notification or the service's open intent (document what you did); capture host-emulator-4.png showing the app resumed with the opened handler's status line.\n" +
      "5) Token proof: the token request must appear in logcat as an attempted call with a clear failure (fake project) surfaced to the app — copy the relevant logcat lines into docs/evidence/native-android/token-proof.log.\n" +
      "6) Reply JSON only: {\"done\":bool,\"summary\":\"...\",\"blockers\":[...],\"evidence\":\"docs/evidence/native-android/...\"}"
    ).catch((e: any) => ({ done: false, blockers: [String(e)] })));
    const judge = agent("QA evaluator·native-android-vision", { system: (await specialist("z-qa-evaluator", "You judge screenshots honestly.")).persona, model: MODELS.flash });
    const vision = asJson(await ask(judge,
      "VISION JUDGE for criterion 5. Read tool: LOOK at every PNG under /Users/hmziq/fo/nsh-firebase/docs/evidence/native-android/. PASS only if: the host shows embedded demo content, a foreground message/notification is visible, a background-delivery or tap/opened-state PNG shows the handler ran (or record honestly what is missing). Write docs/evidence/native-android/vision.md. Reply JSON only: {\"done\":bool,\"pass\":bool,\"looked\":[...],\"issues\":[...],\"summary\":\"...\"}"));
    return {
      criteria: { 5: dev.done === true && vision.pass === true ? "pass" : "unverified" },
      extra: { device: dev.summary ?? "", vision: vision.summary ?? "" },
    };
  },
});

// ---------- 10. demo ----------
areas.push({
  name: "demo",
  short: "demo",
  personaName: "z-nsh-engineer",
  persona: "",
  owns: ["examples/demo/ (except src/screens/monitor.tsx and tests/perfmon-shots.test.tsx, which are perfmon's)", "examples/demo-rs/", "docs/evidence/demo/"],
  commitMessage: "feat(demo): TSX and Rust demo apps showing every product on theme tokens",
  auditFocus:
    "examples/demo (TSX) and examples/demo-rs (Rust) show each product (auth sign-in form via nsh-form, database live list via nsh-query, storage upload with progress, messaging facade status incl. the clear pure-tier failure, analytics event buttons, perf traces + the monitor route importing perfmon's screen file) using the nsh-component-library rules: theme tokens only (no raw colors), one wrapper per nsh/base component, semantic components, virtualized lists for collections; nsh.json capabilities list exactly what is used (network 127.0.0.1 hosts, host firebase.*); headless tests fail-first on manual clocks; the shell resolves early so perfmon's screen can land; the release bundle is built to examples/demo/dist/nsh.",
  gates: [
    { label: "demo tsx gates", cmd: "bash", args: ["scripts/gate-demo.sh"], timeoutMs: 600000 },
    { label: "demo-rs crate", cmd: "bash", args: ["scripts/gate-crate.sh", "demo-rs"], timeoutMs: 600000 },
  ],
  implementAsk: () => `
You are the demo area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Read docs/PROMPT.md, then load the nsh-design-guides and nsh-component-library skills (Skill tool) and skim ~/fo/nsh-query + ~/fo/nsh-form READMEs. THIS FIRST PASS IS THE SHELL ONLY — product screens come in a second pass once the product libraries exist (the workflow will ask you again).
${RULES}
YOUR FILES: examples/demo/** EXCEPT examples/demo/src/screens/monitor.tsx and examples/demo/tests/perfmon-shots.test.tsx (perfmon owns exactly those two; you create the placeholder content of the first so your import compiles — perfmon replaces it wholly later), examples/demo-rs/**, docs/evidence/demo/**.
TASK (shell):
1. examples/demo (TSX app): nsh.json (id dev.nsh.demo; capabilities: network hosts 127.0.0.1, host: ["firebase.*"]); App.tsx with a theme module (semantic tokens only, light+dark, no raw colors), one wrapper per nsh/base component you use, and navigation with routes: home, auth, database, storage, messaging, analytics, monitor. The monitor route imports { MonitorScreen } from "./screens/monitor" — create src/screens/monitor.tsx containing exactly one placeholder export: export function MonitorScreen() { return null; } (perfmon replaces the content later; keep it at zero logic).
2. Link the libraries: cd examples/demo && mkdir -p node_modules && ln -sfn ../../../packages/nsh-firebase node_modules/nsh-firebase; ln -sfn /Users/hmziq/fo/nsh-query/packages/nsh-query node_modules/nsh-query; ln -sfn /Users/hmziq/fo/nsh-form/packages/nsh-form node_modules/nsh-form.
3. Shell routes render honest placeholder screens ("library landing soon" states, themed) EXCEPT they must already be wired to the router; your scoped commands must pass: cd examples/demo && nsh check App.tsx && nsh build App.tsx (every warning fixed).
4. examples/demo-rs: replace core's stub with the real Rust app skeleton (crate name demo-rs, package root examples/demo-rs, nsh.json + Rust root component with the same navigation and placeholder screens, path deps on the nsh-firebase crates and the sibling libraries' crates); cargo check --all-targets must pass. Full screens also come in the second pass.
5. docs/status/demo.md noting the two-pass shape.
Reply JSON only: ${REPORT_FORMAT}`,
  mid: async (a, impl) => {
    // unblock perfmon's screen as soon as the shell exists
    demoShellReady.resolve({ ok: true });
    // wait for the product libraries (never hangs: bounded per dependency)
    const deps = ["core", "auth", "database", "storage", "messaging", "analytics", "perfmon"];
    const landed: string[] = [];
    const missing: string[] = [];
    for (const d of deps) {
      try { await raceTimeout(implFlag(d).promise, 60 * 60 * 1000, "impl of " + d); landed.push(d); }
      catch { missing.push(d); }
    }
    const screens = asJson(await ask(impl,
      "SECOND PASS — product screens. Landed libraries: " + (landed.join(", ") || "none") + ". Missing (their screens must render an honest 'unavailable' state): " + (missing.join(", ") || "none") + ".\n" +
      "In examples/demo (TSX): auth — sign-in/sign-up form via nsh-form rules wired to nsh-firebase/auth with core's emulator config, currentUser signal shown; database — a virtualized live list from nsh-firebase/database listener + a push form; storage — bytes upload with a progress bar from the storage progress signal, download + delete; messaging — token/permission status and the CLEAR pure-tier failure rendered honestly; analytics — buttons logging events with batch status; monitor — perfmon's real screen (already imported). In examples/demo-rs (Rust): the same core interactions headless-testable (sign-in via stubs, list live update, upload progress, clear messaging failure, analytics batch) using the nsh-firebase crates; sibling crates via path deps.\n" +
      "Headless unit tests in examples/demo/tests/demo.test.tsx (manual clocks, stubbed network/host): navigation, auth validation + stubbed sign-in, database live update, storage progress, messaging clear failure, analytics batch, monitor renders with stubbed data. Follow the design guides (tokens, states, interface language).\n" +
      "Then build the release bundle for the native hosts: cd examples/demo && nsh build App.tsx --release --out dist/nsh (do not commit dist/). Re-run: bash scripts/gate-demo.sh; bash scripts/gate-crate.sh demo-rs. Update docs/status/demo.md.\n" +
      "Reply JSON only: {\"done\":bool,\"summary\":\"...\",\"contractLine\":\"...\",\"blockers\":[...],\"evidence\":\"...\",\"bundlePath\":\"<path of dist/nsh or empty>\"}"));
    demoBundleReady.resolve({ ok: screens.done === true, path: screens.bundlePath ?? null });
    return screens.done === true ? "screens + bundle landed (" + screens.bundlePath + ")" : "SCREENS FAILED: " + String(screens.summary ?? "");
  },
  run: async (a, impl) => {
    // vision evidence: macos screenshots + phone (Android) screenshots
    const mac = asJson(await ask(impl,
      "VISION EVIDENCE (demo, macOS): write a screenshot-taking test examples/demo/tests/screenshots.test.tsx (your file; app.screenshot() only works on real-platform tests) that opens the app, walks home → auth → database → storage → analytics → monitor routes, saving PNGs to /Users/hmziq/fo/nsh-firebase/docs/evidence/demo/macos-<route>.png, and run it with nsh test --target macos (log to docs/evidence/demo/macos.log). Every screen must render real content (seeded stubs are fine on macos; say which were stubbed). The monitor route needs the collector running on 127.0.0.1:18742 (start it detached per docs/status/perfmon.md — your own process). Reply JSON only: " + REPORT_FORMAT));
    const phone = await askAndroid(
      "DEVICE TASK (demo, phone target): the repo's Android emulator (AVD nsh-firebase, serial emulator-15557 — boot it exactly as the native areas do if it is not booted; it may already be booted: check adb devices first). Then: cd /Users/hmziq/fo/nsh-firebase/examples/demo && nsh test --target android tests/screenshots.test.tsx (app.screenshot saves PNGs — ensure the test writes to /Users/hmziq/fo/nsh-firebase/docs/evidence/demo/phone-<route>.png), log to docs/evidence/demo/phone.log. NOTE: from inside the Android emulator the Mac's loopback is 10.0.2.2, so any screen hitting the collector must use http://10.0.2.2:18742 there; if a live collector connection is not possible on the emulator, seed the monitor with stubbed data and say so honestly. If nsh cannot find the emulator, fall back to launching the demo in the host app (examples/host-android, installed by the native area) and adb -s emulator-15557 exec-out screencap -p for each route. Reply JSON only: {\"done\":bool,\"summary\":\"...\",\"blockers\":[...],\"evidence\":\"...\"}"
    ).catch((e: any) => ({ done: false, blockers: [String(e)] }));
    const judge = agent("QA evaluator·demo-vision", { system: (await specialist("z-qa-evaluator", "You judge screenshots honestly.")).persona, model: MODELS.flash });
    const vision = asJson(await ask(judge,
      "VISION JUDGE for the demo (supports criteria 7 and 10). Read tool: LOOK at every PNG under /Users/hmziq/fo/nsh-firebase/docs/evidence/demo/. PASS only if: at least four distinct screens render real content on BOTH macos and phone PNGs, the UI uses consistent theme tokens (no obvious unstyled parts), and nothing is a blank/broken render. Write docs/evidence/demo/vision.md (what each PNG shows, verdicts). Reply JSON only: {\"done\":bool,\"pass\":bool,\"looked\":[...],\"issues\":[...],\"summary\":\"...\"}"));
    return {
      criteria: { 7: vision.pass === true ? "pass (demo corroboration)" : "unverified" },
      extra: { macos: mac.summary ?? "", phone: phone.summary ?? "", vision: vision.summary ?? "" },
    };
  },
});

// =====================================================================================
// the fan-out itself
// =====================================================================================
const areaResults: any[] = await Promise.all(areas.map((a) => runArea(a)));
// keep unresolved handoffs from hanging anyone (all areas already joined)
for (const d of [researchDone, coreDone, demoShellReady, demoBundleReady, ...Object.values(implDone)]) {
  try { d.resolve({ ok: false, cancelled: true }); } catch { /* already settled */ }
}

// =====================================================================================
// phase 2 — milestone docs + whole-tree smoke, until zero issues, cap 3
// =====================================================================================
phase("Milestone docs and the whole-tree smoke until zero issues (cap 3)");

const coreRes = areaResults.find((r) => r.name === "core") ?? {};
const researchRes = areaResults.find((r) => r.name === "research") ?? {};
const iosRes = areaResults.find((r) => r.name === "native-ios") ?? {};
const androidRes = areaResults.find((r) => r.name === "native-android") ?? {};
const perfmonRes = areaResults.find((r) => r.name === "perfmon") ?? {};
const demoRes = areaResults.find((r) => r.name === "demo") ?? {};

const milestonePersona = await specialist("z-nsh-engineer", "You are a careful, honest implementer.");
const milestone = agent("nsh engineer·milestone", { system: milestonePersona.persona, model: MODELS.flash });
const smokeSpecs: Spec[] = [
  { label: "rust gates (fmt+clippy+test, all crates)", cmd: "bash", args: ["scripts/gate-workspace.sh"], timeoutMs: 600000 },
  { label: "package unit gates", cmd: "bash", args: ["scripts/test-packages.sh"], timeoutMs: 600000 },
  { label: "demo gates", cmd: "bash", args: ["scripts/test-demo.sh"], timeoutMs: 600000 },
  { label: "emulator sandbox tests (demo-nsh)", cmd: "bash", args: ["scripts/test-emulator.sh"], timeoutMs: 600000 },
  { label: "evidence manifest", cmd: "bash", args: ["scripts/check-evidence.sh"], timeoutMs: 120000 },
];

async function runSmoke(): Promise<{ ok: boolean; failures: { label: string; out: string }[] }> {
  const res = await runSpecs(smokeSpecs);
  if (res.ok) return res;
  board("milestone", "fixing", "smoke failures");
  await ask(milestone,
    "The whole-tree smoke failed. Fix the causes across the tree (all areas have finished; you may touch any file EXCEPT docs/STATUS.md, README.md, docs/setup.md and docs/migration-rnfirebase.md content you are currently writing — coordinate by fixing code and tests). Never weaken, skip or delete a check; a test that cannot pass yet for environmental reasons must fail loudly, not silently.\n" +
    res.failures.map((f) => "SMOKE GATE " + f.label + " FAILED, tail:\n" + f.out).join("\n\n") +
    "\nReply JSON only: " + REPORT_FORMAT);
  return runSpecs(smokeSpecs);
}

let smoke = await runSmoke();
let smokeRounds = 1;
while (!smoke.ok && smokeRounds < 3) {
  smoke = await runSmoke();
  smokeRounds += 1;
}
const smokeConverged = smoke.ok;

// milestone docs (README, STATUS, setup, migration) written by the milestone agent
const milestoneDocs = asJson(await ask(milestone,
  "MILESTONE DOCS in /Users/hmziq/fo/nsh-firebase — you now own four files only: README.md, docs/STATUS.md, docs/setup.md, docs/migration-rnfirebase.md.\n" +
  "1) docs/setup.md: merge docs/setup-ios.md + docs/setup-android.md into one direct setup doc (what the Expo plugin would have automated, done manually here).\n" +
  "2) docs/migration-rnfirebase.md: a table mapping each React Native Firebase call a user knows (from ~/fo/.refs/react-native-firebase packages' lib/ APIs) to its nsh-firebase equivalent (TSX subpath and Rust crate), noting tier differences honestly.\n" +
  "3) README.md (criterion 10): from a clean clone to `bun install`, the one emulator-test command (bun run test:emulator), and a running demo (nsh run in examples/demo + the hosts), plus the node_modules symlink step for siblings; direct, concrete, no marketing filler (antislop-copy rules).\n" +
  "4) docs/STATUS.md: assemble from docs/status/*.md (every area wrote one) + the evidence index under docs/evidence/; mark EVERY satisfaction criterion 1-10 with pass/unverified and the evidence path; a claim with no evidence is unverified — keep it honest.\n" +
  "Reply JSON only: " + REPORT_FORMAT));

const milestoneCommit = await commitRequest(
  ["README.md", "docs/STATUS.md", "docs/setup.md", "docs/migration-rnfirebase.md"],
  "docs: milestone README, STATUS with evidence, setup and the RNFB migration note"
).catch(() => ({ done: false, summary: "commit queue lost" }));

// honest clean-checkout proof for criterion 3 (one documented command from a clean clone)
const cleanProof = await world.run("bash", ["-c",
  "rm -rf /tmp/nsh-firebase-clean && git clone -q --no-hardlinks /Users/hmziq/fo/nsh-firebase /tmp/nsh-firebase-clean && cd /tmp/nsh-firebase-clean && bun install --frozen-lockfile >/tmp/nsh-firebase-clean-install.log 2>&1 && bun run test:emulator >/tmp/nsh-firebase-clean-test.log 2>&1 && echo CLEAN_PROOF_OK || (tail -20 /tmp/nsh-firebase-clean-install.log /tmp/nsh-firebase-clean-test.log; exit 1)"], { timeoutMs: 600000 });

// =====================================================================================
// phase 3 — stretch, only once criteria 1-8 hold
// =====================================================================================
phase("Stretch areas, only if satisfaction criteria 1-8 hold");

const c = (n: number): string => {
  switch (n) {
    case 1: return researchRes.status === "done" ? "pass" : "unverified";
    case 2: return (smokeConverged && specPassed(smoke, "rust gates") && specPassed(smoke, "package unit gates")) ? "pass" : "unverified";
    case 3: return (specPassed(smoke, "emulator sandbox tests (demo-nsh)") && gateOk(cleanProof)) ? "pass" : "unverified";
    case 4: return iosRes.criteria?.["4"] === "pass" ? "pass" : "unverified";
    case 5: return androidRes.criteria?.["5"] === "pass" ? "pass" : "unverified";
    case 6: return (specPassed(smoke, "evidence manifest") && perfmonRes.criteria?.["7"] !== undefined) ? "pass" : "unverified";
    case 7: return (perfmonRes.criteria?.["7"] === "pass" || perfmonRes.criteria?.["7"] === "pass (demo corroboration)") ? "pass" : "unverified";
    case 8: return coreRes.criteria?.["8"] === "pass" ? "pass" : "unverified";
    default: return "unverified";
  }
};
function specPassed(sm: { failures: { label: string }[] }, label: string): boolean {
  return !sm.failures.some((f) => f.label === label);
}
const criteria18 = [1, 2, 3, 4, 5, 6, 7, 8].map((n) => [n, c(n)] as const);
const stretchAllowed = criteria18.every(([, v]) => v === "pass");

let stretchResult: any = { name: "stretch", status: "skipped", summary: stretchAllowed ? "" : "criteria 1-8 not all green: " + criteria18.filter(([, v]) => v !== "pass").map(([n, v]) => "criterion " + n + "=" + v).join(", "), criteria: {}, extra: {} };
if (stretchAllowed) {
  board("stretch", "implementing", "firestore, remote-config, crashlytics, installations, app-check, functions");
  const stretchPersona = await specialist("z-nsh-engineer", "You are a careful, honest implementer.");
  const stretchImpl = agent("nsh engineer·stretch", { system: stretchPersona.persona, model: MODELS.flash });
  const sres = asJson(await ask(stretchImpl,
    "You are the stretch area of the nsh-firebase build in /Users/hmziq/fo/nsh-firebase. Criteria 1-8 hold, so per docs/PROMPT.md area 11 you now add, IN THIS ORDER, as many as one sitting honestly allows: Firestore, Remote Config, Crashlytics, Installations, App Check, Functions. Each product: fill the stub crate crates/nsh-firebase-<product>/ and the stub subpath packages/nsh-firebase/src/<product>/ (both exist; you own them now), same behavior in Rust and TS, working against the emulator suite where the product has one (Firestore and Functions do; Auth-style token flows via Installations/App Check REST per docs/research.md), each with an HONEST tier table (pure tier over REST vs native tier through the facade) in docs/status/stretch.md (one status file for all six, clearly sectioned). Crashlytics has NO pure-tier path that is honest — its table must say so and its crate must be a clearly-failing facade, not a fake recorder. Fail-first manual-clock unit tests for every real implementation; emulator sandbox tests for Firestore/Functions via the established runner conventions. Finish products one at a time, re-running their scoped gates (bash scripts/gate-crate.sh nsh-firebase-<product> and bash scripts/gate-tsx.sh with your own test entry) after each; stop cleanly when the sitting is full and report exactly which products are done vs stubbed.\n" + RULES +
    "\nReply JSON only: " + REPORT_FORMAT));
  const saudit = await auditLoop({ name: "stretch", short: "stretch", persona: stretchPersona.persona, owns: ["crates/nsh-firebase-firestore/", "crates/nsh-firebase-remote-config/", "crates/nsh-firebase-crashlytics/", "crates/nsh-firebase-installations/", "crates/nsh-firebase-app-check/", "crates/nsh-firebase-functions/", "packages/nsh-firebase/src/firestore/", "packages/nsh-firebase/src/remote-config/", "packages/nsh-firebase/src/crashlytics/", "packages/nsh-firebase/src/installations/", "packages/nsh-firebase/src/app-check/", "packages/nsh-firebase/src/functions/"], auditFocus: "honest tier tables, real APIs only, fail-first tests, gates pass for every product attempted", gates: [] } as AreaDef, stretchImpl);
  const sgates = await gateLoop({ name: "stretch", short: "stretch", persona: "", owns: [], auditFocus: "", gates: [
    { label: "stretch crates", cmd: "bash", args: ["-c", "for p in firestore remote-config crashlytics installations app-check functions; do bash scripts/gate-crate.sh nsh-firebase-$p || exit 1; done"], timeoutMs: 600000 },
    { label: "workspace still green", cmd: "bash", args: ["scripts/gate-workspace.sh"], timeoutMs: 600000 },
  ] } as AreaDef, stretchImpl);
  await commitRequest(["crates/nsh-firebase-firestore", "crates/nsh-firebase-remote-config", "crates/nsh-firebase-crashlytics", "crates/nsh-firebase-installations", "crates/nsh-firebase-app-check", "crates/nsh-firebase-functions", "packages/nsh-firebase/src/firestore", "packages/nsh-firebase/src/remote-config", "packages/nsh-firebase/src/crashlytics", "packages/nsh-firebase/src/installations", "packages/nsh-firebase/src/app-check", "packages/nsh-firebase/src/functions", "docs/status/stretch.md"], "feat(stretch): firestore, remote-config and the rest as evidence allows").catch(() => ({ done: false, summary: "commit queue lost" }));
  stretchResult = {
    name: "stretch",
    status: sres.done !== false && saudit.converged && sgates.converged ? "done" : "failed",
    summary: String(sres.summary ?? "") + (saudit.converged ? "" : " | audit did not converge") + (sgates.converged ? "" : " | gates did not converge"),
    criteria: {}, extra: { auditRounds: saudit.rounds, gateRounds: sgates.rounds },
  };
  board("stretch", stretchResult.status === "done" ? "done" : "failed", String(stretchResult.summary).slice(0, 120));
}

// =====================================================================================
// final return
// =====================================================================================
const conclusionLines: string[] = [];
conclusionLines.push("nsh-firebase build finished. Areas: " + areaResults.map((r) => r.name + "=" + r.status).join(", ") + ", stretch=" + stretchResult.status + ".");
conclusionLines.push("Whole-tree smoke: " + (smokeConverged ? "converged" : "DID NOT CONVERGE after 3 rounds — failures: " + smoke.failures.map((f) => f.label).join("; ")) + ". Clean-checkout emulator proof: " + (gateOk(cleanProof) ? "passed (bun run test:emulator from a fresh clone)" : "NOT proven from a clean clone — see milestone logs") + ".");
conclusionLines.push("Criteria: " + criteria18.map(([n, v]) => n + ":" + v).join(" ") + (stretchAllowed ? " (stretch ran)" : " (stretch skipped per the brief)"));
if (String(milestoneDocs.summary ?? "").length > 0) conclusionLines.push("Milestone docs: " + String(milestoneDocs.summary).slice(0, 200));

return {
  conclusion: conclusionLines.join(" "),
  areas: [...areaResults, stretchResult].map((r: any) => ({
    name: r.name, status: r.status, summary: String(r.summary ?? "").slice(0, 600),
    rounds: r.auditRounds !== undefined ? { audit: r.auditRounds, gates: r.gateRounds } : undefined,
    blockers: Array.isArray(r.blockers) ? r.blockers.slice(0, 4) : [],
    evidence: r.evidence ?? undefined, criteria: r.criteria,
  })),
  notes:
    "All subagents pinned GLM-5.3-Flash$high per the brief; no main-tier model used anywhere. " +
    "Phases parsed from the brief: implement, audit (until zero issues, cap 3), unit tests (scoped per area), native device testing (native-ios, native-android, demo phone, perfmon phone), vision (demo, perfmon, native evidence judging), whole-tree smoke (until zero issues, cap 3) + gated stretch. " +
    "Research gates the native areas' API choices via a resolved handoff; core gates everything that imports it; demo's shell gates perfmon's screen file; demo's bundle gates the native device phases. " +
    "Emulator suite runs on demo-nsh with non-default ports (auth 19099, database 19000, storage 19199) started by core; devices are repo-owned (simulator nsh-firebase-ios, AVD nsh-firebase on emulator-15557) and all device work serialized through two verifier queues. " +
    "Commits made on main via a single committer queue with Conventional Commits. " +
    "Launcher note: the dynamic-workflows skill could not be loaded from the launcher's subagent context (Skill tool refused with 'Skill is not allowed for subagent'); the script follows the authoring contract from the z-workflow skill, references/proven-patterns.md and references/specialist-personas.md instead, and could not be submitted via CreateWorkflow from this context — returned as ready for the coordinator to submit.",
};
