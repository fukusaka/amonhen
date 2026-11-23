// Renders pele's test corpus with pele itself, for the Rust port's tests (crates/core/tests/data/pele).
// TZ=UTC node --no-warnings --import ./register.mjs corpus.mjs <pele checkout> <out dir> [corpus name prefix]
// (TZ=UTC: gantt counts in local time, which the Rust port takes to be UTC. `now` is fixed, as in
// crates/pele/tests/oracles.rs: gantt draws a today line and puts time-only dates on today.)
// register.mjs/hooks.mjs let node run pele's TypeScript source without installing anything: they resolve
// `.js` imports to `.ts`, turn `const enum`s and constructor parameter properties into plain JS, and strip
// the types with node's own stripTypeScriptTypes.
const [,, src, out, only = ''] = process.argv;
const corpusDir = src + '/tests/corpus';
const pele = await import(src + '/src/index.ts');
const fs = await import('node:fs');
const path = await import('node:path');
fs.mkdirSync(out, { recursive: true });
const NOW = Date.UTC(2000, 0, 1);
const summary = {};
for (const f of fs.readdirSync(corpusDir).filter((f) => f.endsWith('.json') && f.startsWith(only)).sort()) {
  const name = f.replace(/\.json$/, '');
  const inputs = JSON.parse(fs.readFileSync(path.join(corpusDir, f), 'utf8'));
  let ok = 0, err = 0, ms = 0;
  inputs.forEach((entry, i) => {
    const input = typeof entry === "string" ? entry : entry.src;
    const base = path.join(out, `${name}-${String(i).padStart(3, '0')}`);
    fs.writeFileSync(base + '.mmd', input);
    try {
      pele.render(input, { now: NOW });
      const t = performance.now();
      const r = pele.render(input, { now: NOW });
      ms += performance.now() - t;
      fs.writeFileSync(base + '.svg', r.svg);
      ok++;
    } catch (e) {
      fs.writeFileSync(base + '.err', String(e && e.message || e));
      err++;
    }
  });
  summary[name] = { ok, err, avgMs: +(ms / Math.max(ok, 1)).toFixed(3) };
}
console.log(JSON.stringify(summary, null, 1));
