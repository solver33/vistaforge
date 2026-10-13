// Undo and redo over snapshots of the settings, stored as JSON text.

export class History {
  private past: string[] = [];
  private future: string[] = [];
  private current: string;

  constructor(
    initial: string,
    private readonly limit = 200,
  ) {
    this.current = initial;
  }

  /** Records `state` as a step, unless it is what was recorded last. */
  commit(state: string): void {
    if (state === this.current) return;
    this.past.push(this.current);
    if (this.past.length > this.limit) this.past.shift();
    this.current = state;
    this.future = [];
  }

  /** Forgets all steps and starts again from `state` (used when a file is opened). */
  reset(state: string): void {
    this.past = [];
    this.future = [];
    this.current = state;
  }

  undo(): string | null {
    const prev = this.past.pop();
    if (prev === undefined) return null;
    this.future.push(this.current);
    this.current = prev;
    return prev;
  }

  redo(): string | null {
    const next = this.future.pop();
    if (next === undefined) return null;
    this.past.push(this.current);
    this.current = next;
    return next;
  }

  get canUndo(): boolean {
    return this.past.length > 0;
  }

  get canRedo(): boolean {
    return this.future.length > 0;
  }
}
