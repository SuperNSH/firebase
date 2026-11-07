// The event bus, shared by every product. Mirrors
// crates/nsh-firebase-core/src/bus.rs: listeners run in subscription
// order over a snapshot taken at emit, and an unsubscribe during an emit
// affects only later emits.

export type Listener = (payload: unknown) => void;

export class EventBus {
  private readonly byEvent = new Map<string, Map<number, Listener>>();
  private nextId = 1;

  // Subscribes to `event`; the returned function unsubscribes.
  on(event: string, listener: Listener): () => void {
    const id = this.nextId++;
    let byId = this.byEvent.get(event);
    if (!byId) {
      byId = new Map();
      this.byEvent.set(event, byId);
    }
    byId.set(id, listener);
    return () => this.off(id);
  }

  // Removes one listener; one that never subscribed changes nothing.
  off(id: number): void {
    for (const byId of this.byEvent.values()) byId.delete(id);
  }

  // Fans `payload` out to the subscribers of `event`, in subscription
  // order, over the snapshot taken here.
  emit(event: string, payload: unknown): void {
    const snapshot = [...(this.byEvent.get(event)?.values() ?? [])];
    for (const listener of snapshot) listener(payload);
  }
}
