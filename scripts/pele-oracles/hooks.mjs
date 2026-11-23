import { stripTypeScriptTypes } from 'node:module';
import { readFile } from 'node:fs/promises';
export async function resolve(specifier, context, next) {
  try { return await next(specifier, context); }
  catch (e) {
    if (specifier.endsWith('.js') && (specifier.startsWith('.') || specifier.startsWith('/')))
      return next(specifier.slice(0, -3) + '.ts', context);
    throw e;
  }
}
function enums(src) {
  return src.replace(/(export\s+)?(?:const\s+)?enum\s+(\w+)\s*\{([^}]*)\}/g, (_, exp, name, body) => {
    let next = 0;
    const items = [];
    for (let part of body.replace(/\/\/[^\n]*/g, '').split(',')) {
      part = part.trim();
      if (!part) continue;
      const m = part.match(/^(\w+)\s*(?:=\s*(.+))?$/s);
      if (!m) throw new Error('enum item ' + part);
      let value;
      if (m[2] !== undefined) {
        value = m[2].trim();
        const n = Number(value);
        next = Number.isNaN(n) ? next : n + 1;
      } else {
        value = String(next++);
      }
      items.push(`${m[1]}: ${value}`);
    }
    return `${exp ?? ''}const ${name} = Object.freeze({ ${items.join(', ')} });`;
  });
}
function params(src) {
  let out = '';
  let i = 0;
  for (;;) {
    const at = src.indexOf('constructor(', i);
    if (at < 0) return out + src.slice(i);
    let j = at + 'constructor('.length, depth = 1;
    while (depth > 0) {
      const c = src[j++];
      if ('([{<'.includes(c)) depth++;
      else if (')]}>'.includes(c)) depth--;
    }
    const list = src.slice(at + 'constructor('.length, j - 1);
    const parts = [];
    let d = 0, cur = '';
    for (const c of list) {
      if ('([{<'.includes(c)) d++;
      if (')]}>'.includes(c)) d--;
      if (c === ',' && d === 0) { parts.push(cur); cur = ''; } else cur += c;
    }
    if (cur.trim()) parts.push(cur);
    const assigns = [];
    const cleaned = parts.map((p) => {
      const m = p.match(/^\s*((?:(?:private|public|protected|readonly)\s+)+)(\w+)([\s\S]*)$/);
      if (!m) return p;
      assigns.push(`this.${m[2]} = ${m[2]};`);
      return ' ' + m[2] + m[3];
    });
    const brace = src.indexOf('{', j);
    // A derived class must call super() before it touches this.
    const sup = /^\s*super\([^;]*\);/.exec(src.slice(brace + 1));
    const cut = sup ? brace + 1 + sup[0].length : brace + 1;
    out += src.slice(i, at) + 'constructor(' + cleaned.join(',') + src.slice(j - 1, cut) + assigns.join(' ');
    i = cut;
  }
}
export async function load(url, context, next) {
  if (url.endsWith('.ts')) {
    const src = params(enums(await readFile(new URL(url), 'utf8')));
    const code = stripTypeScriptTypes(src, { sourceUrl: url });
    return { format: 'module', source: code, shortCircuit: true };
  }
  if (url.endsWith('.json')) {
    const obj = JSON.parse(await readFile(new URL(url), 'utf8'));
    const named = Object.keys(obj).filter((k) => /^[A-Za-z_$][\w$]*$/.test(k)).map((k) => `export const ${k} = ${JSON.stringify(obj[k])};`).join('\n');
    return { format: 'module', source: named + `\nexport default ${JSON.stringify(obj)};`, shortCircuit: true };
  }
  return next(url, context);
}
