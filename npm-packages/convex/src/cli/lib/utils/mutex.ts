export class Mutex {
  private tail: Promise<void> = Promise.resolve();

  async runExclusive<T>(fn: () => Promise<T>): Promise<T> {
    const result = this.tail.then(fn);
    // Reserve the next position before running fn, and allow the queue to
    // continue even if fn rejects or throws synchronously.
    this.tail = result.then(
      () => {},
      () => {},
    );
    return result;
  }
}
