export function formatInt(n: number): string {
  return Math.round(n).toLocaleString('en-US');
}

export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '—';
  if (seconds < 10) return `${seconds.toFixed(1)} s`;
  if (seconds < 60) return `${Math.round(seconds)} s`;
  const m = Math.floor(seconds / 60);
  const s = Math.round(seconds % 60);
  return s === 0 ? `${m} min` : `${m} min ${s} s`;
}

export function errorText(e: unknown): string {
  const text = e instanceof Error ? e.message : String(e);
  // The core's messages start lower-case; sentences in the UI don't.
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** The last part of a path, for compact display. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Shortens a long path in the middle, keeping the drive and the last folders. */
export function shortPath(path: string, max = 40): string {
  if (path.length <= max) return path;
  const keepEnd = Math.ceil(max * 0.62);
  const keepStart = max - keepEnd - 1;
  return `${path.slice(0, keepStart)}…${path.slice(path.length - keepEnd)}`;
}
