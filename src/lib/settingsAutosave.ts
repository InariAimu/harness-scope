/** Serialize writes and coalesce edits while a request is running. A stale
 * response/failure must never replace a newer edit of the same field. */
export class SettingsAutosave<T extends object> {
  private pending: Partial<T> = {};
  private revisions = new Map<keyof T, number>();
  private running = false;
  private sequence = 0;

  constructor(private callbacks: {
    save: (patch: Partial<T>) => Promise<T>;
    saved: (latest: Partial<T>, result: T) => void;
    failed: (latest: Partial<T>, error: unknown) => void;
    busy: (busy: boolean) => void;
  }) {}

  enqueue(patch: Partial<T>) {
    for (const key of Object.keys(patch) as (keyof T)[]) {
      this.pending[key] = patch[key];
      this.revisions.set(key, ++this.sequence);
    }
    if (!this.running) void this.drain();
  }

  private async drain() {
    this.running = true;
    this.callbacks.busy(true);
    while (Object.keys(this.pending).length) {
      const patch = this.pending;
      this.pending = {};
      const revisions = new Map(this.revisions);
      const latest = () => Object.fromEntries(
        Object.entries(patch).filter(([key]) =>
          revisions.get(key as keyof T) === this.revisions.get(key as keyof T)),
      ) as Partial<T>;
      try {
        const result = await this.callbacks.save(patch);
        this.callbacks.saved(latest(), result);
      } catch (error) {
        this.callbacks.failed(latest(), error);
      }
    }
    this.running = false;
    this.callbacks.busy(false);
  }
}
