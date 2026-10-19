// How much work to hand the graphics card at once. Too much in one go and the window stops
// keeping up with the mouse, or Windows decides the card has hung (after two seconds) and
// resets it; too little and the render crawls. Each batch's time decides the next one's size.

export interface PacerOptions {
  /** What one batch should take on the graphics card, in milliseconds. */
  targetMs: number;
  /** The first batch, before anything has been timed. */
  start: number;
  /** The smallest batch. */
  min: number;
}

export class Pacer {
  /** Units of work for the next full batch. */
  size: number;

  constructor(private readonly o: PacerOptions) {
    this.size = o.start;
  }

  /** The next batch: at most `max` units. */
  next(max: number): number {
    return Math.max(Math.min(this.o.min, max), Math.min(max, this.size));
  }

  /**
   * A batch of `units` took `ms`. When `quantum` is above zero the time is only known to the
   * nearest frame or two (it is how long until the card was seen to be done), so a short
   * time says the batch was not slow, not how fast it was.
   */
  took(units: number, ms: number, quantum: number, max: number): void {
    const t = this.o.targetMs;
    let f: number;
    if (quantum <= 0) f = Math.min(2, Math.max(0.5, t / Math.max(ms, 0.05)));
    else if (ms > 1.5 * t) f = Math.max(0.4, t / ms);
    else if (ms <= Math.max(t, 2 * quantum)) f = 1.5;
    else f = 1;
    // A short last batch (the end of a sample) says nothing about a full one, except when slow.
    const next = f < 1 ? Math.min(this.size, units * f) : units >= this.size ? units * f : this.size;
    this.size = Math.max(this.o.min, Math.min(max, next));
  }
}
