// World folder names that work on Windows. The bridge checks the same rules before export
// (app/bridge/src/paths.rs, `check_world_name`).

const RESERVED = new Set([
  'CON', 'PRN', 'AUX', 'NUL',
  ...Array.from({ length: 9 }, (_, i) => `COM${i + 1}`),
  ...Array.from({ length: 9 }, (_, i) => `LPT${i + 1}`),
]);

/** Why `name` can't be a world folder name, or null when it can. */
export function worldNameProblem(name: string): string | null {
  if (name.trim() === '') return 'The world needs a name.';
  if ([...name].length > 80) return 'Keep the name under 80 characters.';
  const bad = [...name].find((c) => c.charCodeAt(0) < 32 || '<>:"/\\|?*'.includes(c));
  if (bad !== undefined) {
    return bad.charCodeAt(0) < 32
      ? 'The name contains a control character.'
      : `The name can't contain ${bad}`;
  }
  if (name.startsWith(' ') || name.endsWith(' ') || name.endsWith('.')) {
    return "The name can't start or end with a space, or end with a dot.";
  }
  const stem = name.split('.')[0].trim().toUpperCase();
  if (RESERVED.has(stem)) return `Windows doesn't allow a folder named ${name}.`;
  return null;
}
