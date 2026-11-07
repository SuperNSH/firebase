// The storage exercise UI the tests drive: the resumable upload (its
// progress log, pause/resume/cancel, resume by a saved session id) and
// the reads (download, download URL, metadata, list paging, delete, the
// missing object's stable error). Mirrors the components in
// crates/nsh-firebase-storage/tests/storage_ui.rs. All test ids and
// lines carry the `st` prefix, so the core exercise app can share the
// entry without its selectors colliding.
import { effect, mount, signal, Text, View, type Signal } from "nsh";
import { demoOptions, initializeApp } from "../src/core";
import { FirebaseStorage, UploadTask } from "../src/storage";

const PATH = "st/big.bin";
// 600_000 bytes: two full 256 KiB slices and a final 75_712 one.
const PAYLOAD_LEN = 600_000;

function storageFor(): FirebaseStorage {
  const app = initializeApp(demoOptions(), "storage-ui");
  return new FirebaseStorage(app);
}

function uploadNow(
  task: Signal<UploadTask | null>,
  line: Signal<string>,
  session: string | null,
): void {
  const reference = storageFor().ref(PATH);
  // ASCII on purpose: the test's stub reads slice bodies as text, so the
  // string length must equal the byte count.
  const payload = new Uint8Array(PAYLOAD_LEN).fill(0x41);
  const newTask =
    session === null ? reference.upload(payload) : reference.resumeUpload(payload, session);
  task.set(newTask);
  void newTask.result.then(
    (metadata) => line.set(`st: finished ok ${metadata.fullPath} size=${metadata.size ?? 0}`),
    (error: { code: string }) => line.set(`st: finished err ${error.code}`),
  );
}

export function StorageExerciseApp() {
  const line = signal("st: storage ready");
  const progressLog = signal("st: idle");
  const task = signal<UploadTask | null>(null);

  // Re-runs when the task is replaced or its progress changes; appends
  // every transition it sees.
  effect(() => {
    const current = task();
    if (current === null) {
      progressLog.set("st: idle");
      return;
    }
    const snapshot = current.progress();
    progressLog.update(
      (text) => `${text} ${snapshot.bytesTransferred}/${snapshot.totalBytes} ${snapshot.state}`,
    );
  });

  const runRead = (work: (storage: FirebaseStorage) => Promise<string>) => {
    void (async () => {
      try {
        line.set(await work(storageFor()));
      } catch (error) {
        line.set(`st: err ${(error as { code: string }).code}`);
      }
    })();
  };

  // What resumeUpload needs saved across a process death: the session
  // URI the server issued.
  let savedSession: string | null = null;
  const saveSession = () => {
    savedSession = task()?.sessionUrl ?? null;
    line.set(`st: session ${savedSession ?? ""}`);
  };

  const button = (testID: string, label: string, onPress: () => void) => (
    <View testID={testID} role="button" onPress={onPress} class="bg-primary p-1 px-2">
      <Text class="text-primary-foreground">{label}</Text>
    </View>
  );

  // The buttons sit in a compact grid: the core exercise app shares this
  // entry, and everything must stay on screen for the test steps.
  return (
    <View class="flex-1 gap-1 bg-background p-2">
      <Text testID="st-progress">{progressLog()}</Text>
      <Text testID="st-state">{line()}</Text>
      <View class="flex-row flex-wrap gap-1">
        {button("st-upload", "st upload", () => uploadNow(task, line, null))}
        {button("st-save", "st save", saveSession)}
        {button("st-resume", "st resume id", () => uploadNow(task, line, savedSession))}
        {button("st-pause", "st pause", () => task()?.pause())}
        {button("st-cancel", "st cancel", () => task()?.cancel())}
        {button(
          "st-download",
          "st download",
          () =>
            runRead(async (storage) => {
              const bytes = await storage.ref("st/file.txt").download();
              const got = new TextDecoder().decode(bytes);
              if (got !== "file-bytes") return "st: download MISMATCH";
              return `st: download ok ${bytes.length}`;
            }),
        )}
        {button(
          "st-auth-download",
          "st auth dl",
          () =>
            runRead(async (shared) => {
              const storage = shared.withAuth(() => "st-id-token");
              const bytes = await storage.ref("st/secret.txt").download();
              return `st: auth download ok ${bytes.length}`;
            }),
        )}
        {button(
          "st-url",
          "st url",
          () =>
            runRead(async (storage) => {
              const url = await storage.ref("st/file.txt").downloadUrl();
              return `st: url ${url}`;
            }),
        )}
        {button(
          "st-metadata",
          "st metadata",
          () =>
            runRead(async (storage) => {
              const metadata = await storage.ref("st/file.txt").getMetadata();
              return `st: metadata ct=${metadata.contentType ?? "-"} size=${metadata.size ?? 0}`;
            }),
        )}
        {button(
          "st-patch",
          "st patch",
          () =>
            runRead(async (storage) => {
              const metadata = await storage.ref("st/file.txt").updateMetadata({
                contentType: "text/updated",
              });
              return `st: patched ct=${metadata.contentType ?? "-"} gen=${metadata.metageneration ?? "-"}`;
            }),
        )}
        {button(
          "st-list",
          "st list",
          () =>
            runRead(async (storage) => {
              const first = await storage.ref("st").list(2);
              const second = await storage
                .ref("st")
                .list(2, first.nextPageToken === null ? undefined : first.nextPageToken);
              return `st: page1=${first.items.length} next=${first.nextPageToken ?? "-"} page2=${second.items.length}`;
            }),
        )}
        {button(
          "st-delete",
          "st delete",
          () =>
            runRead(async (storage) => {
              await storage.ref("st/file.txt").delete();
              return "st: deleted ok";
            }),
        )}
        {button(
          "st-missing",
          "st missing",
          () =>
            runRead(async (storage) => {
              const metadata = await storage.ref("st/gone.txt").getMetadata();
              return `st: metadata ct=${metadata.contentType ?? "-"}`;
            }),
        )}
        {button(
          "st-sandbox",
          "st sandbox",
          () =>
            runRead(async (storage) => {
              // The labelled sandbox flow (tests/emulator): a small
              // fixture, progress, its URL, the bytes back, then delete.
              const fixture = "nsh-firebase storage sandbox fixture";
              const bytes = Uint8Array.from(fixture, (char) => char.charCodeAt(0));
              const reference = storage.ref("st/sandbox-fixture.txt");
              const upload = reference.upload(bytes, { contentType: "text/plain" });
              // Route the task through the same signal the progress log
              // tracks, so the flow's progress events show on screen.
              task.set(upload);
              const metadata = await upload.result;
              const url = await reference.downloadUrl();
              const back = await reference.download();
              const roundTrip = new TextDecoder().decode(back) === fixture;
              await reference.delete();
              return `st: sandbox ok size=${metadata.size ?? 0} roundtrip=${roundTrip} token=${url.includes("token=")}`;
            }),
        )}
      </View>
    </View>
  );
}

mount(StorageExerciseApp);
