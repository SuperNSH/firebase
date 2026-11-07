// The Cloud Storage screen: a resumable upload with its progress bar fed
// by the task's progress signal, then download and delete of the same
// object.
import { Scroll, Text, View, signal } from "nsh";
import { demoStorage } from "../firebase";
import type { UploadTask } from "nsh-firebase/storage";
import { Button } from "../../ui/button";
import { Progress } from "../../ui/progress";

// 600_000 bytes: two full 256 KiB slices and a rest, so the bar steps.
const PAYLOAD_LEN = 600_000;
const OBJECT_PATH = "demo/notes.bin";

// ASCII fill on purpose: the test's stub reads slice bodies as text.
function payload(): Uint8Array {
  return new Uint8Array(PAYLOAD_LEN).fill(0x41);
}

export function StorageScreen() {
  const storage = demoStorage();
  const task = signal<UploadTask | null>(null);
  const line = signal("No object uploaded yet.");
  const objectName = signal<string | null>(null);

  const start = () => {
    if (task() !== null) return;
    const reference = storage.ref(OBJECT_PATH);
    const upload = reference.upload(payload());
    task.set(upload);
    void upload.result.then(
      (metadata) => {
        line.set(`Uploaded ${metadata.name} (${metadata.size ?? PAYLOAD_LEN} bytes).`);
        objectName.set(metadata.name);
        task.set(null);
      },
      (error: { message?: string }) => {
        line.set(`Upload failed: ${error.message ?? "unknown error"}`);
        task.set(null);
      },
    );
  };

  const download = () => {
    void storage
      .ref(OBJECT_PATH)
      .download()
      .then(
        (bytes) => line.set(`Downloaded ${bytes.length} bytes.`),
        (error: { message?: string }) => line.set(`Download failed: ${error.message ?? "unknown error"}`),
      );
  };

  const remove = () => {
    void storage
      .ref(OBJECT_PATH)
      .delete()
      .then(() => {
        line.set("Deleted.");
        objectName.set(null);
      });
  };

  const progress = () => task()?.progress() ?? null;

  return (
    <Scroll class="flex-1 bg-background">
      <View class="gap-4 p-4 pb-[env(safe-area-inset-bottom)]">
        <View class="gap-2">
          <Text class="text-sm text-foreground">{line()}</Text>
          <If when={progress() !== null}>
            <View class="gap-1" testID="storage-progress">
              <Progress
                value={(progress()!.bytesTransferred / progress()!.totalBytes) * 100}
                accessibilityLabel="Upload progress"
              />
              <Text class="text-xs text-muted-foreground">
                {`${progress()!.bytesTransferred} of ${progress()!.totalBytes} bytes · ${progress()!.state}`}
              </Text>
            </View>
          </If>
        </View>
        <View class="flex-row flex-wrap gap-2">
          <Button
            accessibilityLabel="Upload"
            disabled={task() !== null}
            testID="storage-upload"
            onPress={start}
          >
            <Text class="text-sm font-medium text-primary-foreground">Upload 600 kB</Text>
          </Button>
          <Button
            variant="outline"
            accessibilityLabel="Download"
            disabled={objectName() === null}
            testID="storage-download"
            onPress={download}
          >
            <Text class="text-sm font-medium text-foreground">Download</Text>
          </Button>
          <Button
            variant="outline"
            accessibilityLabel="Delete"
            disabled={objectName() === null}
            testID="storage-delete"
            onPress={remove}
          >
            <Text class="text-sm font-medium text-foreground">Delete</Text>
          </Button>
        </View>
        <Text class="text-xs text-muted-foreground">
          Resumable upload to the demo-nsh emulator on 127.0.0.1:19199, in 256 KiB slices.
        </Text>
      </View>
    </Scroll>
  );
}
