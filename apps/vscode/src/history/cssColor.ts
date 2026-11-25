/** CSS の色（`rgb(…)`・`rgba(…)`）を `#rrggbb` に（透明なものは undefined） */
export function hexColor(css: string): string | undefined {
  const m = /^rgba?\(\s*(\d+(?:\.\d+)?)[,\s]+(\d+(?:\.\d+)?)[,\s]+(\d+(?:\.\d+)?)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/.exec(css.trim());
  if (!m) return undefined;
  const alpha = m[4] === undefined ? 1 : m[4].endsWith('%') ? Number(m[4].slice(0, -1)) / 100 : Number(m[4]);
  if (alpha === 0) return undefined;
  return `#${[m[1], m[2], m[3]].map((v) => Math.round(Math.min(255, Number(v))).toString(16).padStart(2, '0')).join('')}`;
}
