import { describe, expect, it } from 'vitest';
import { Pacer } from './pacer';

describe('Pacer', () => {
  it('grows towards the target with exact times, at most doubling', () => {
    const p = new Pacer({ targetMs: 30, start: 100, min: 4 });
    p.took(100, 3, 0, 10_000);
    expect(p.size).toBe(200);
    p.took(200, 20, 0, 10_000);
    expect(p.size).toBe(300);
  });

  it('shrinks after a slow batch, and never below the minimum', () => {
    const p = new Pacer({ targetMs: 30, start: 1000, min: 4 });
    p.took(1000, 300, 0, 10_000);
    expect(p.size).toBe(500);
    p.took(500, 3000, 16, 10_000);
    expect(p.size).toBe(200);
    for (let i = 0; i < 20; i++) p.took(p.size, 5000, 16, 10_000);
    expect(p.size).toBe(4);
  });

  it('with frame-rounded times, grows gently while batches are not seen to be slow', () => {
    const p = new Pacer({ targetMs: 30, start: 100, min: 4 });
    p.took(100, 33, 16.7, 10_000);
    expect(p.size).toBe(150);
    p.took(150, 40, 16.7, 10_000);
    expect(p.size).toBe(150);
    p.took(150, 50, 16.7, 10_000);
    expect(p.size).toBe(90);
  });

  it('keeps its size after a short last batch, unless that was slow', () => {
    const p = new Pacer({ targetMs: 30, start: 400, min: 4 });
    p.took(10, 2, 0, 10_000);
    expect(p.size).toBe(400);
    p.took(10, 300, 0, 10_000);
    expect(p.size).toBe(5);
  });

  it('stays within the largest batch there is', () => {
    const p = new Pacer({ targetMs: 30, start: 100, min: 4 });
    p.took(100, 1, 0, 120);
    expect(p.size).toBe(120);
    expect(p.next(50)).toBe(50);
    expect(p.next(2)).toBe(2);
  });
});
