#!/usr/bin/env node
// Run the SketchUp Ruby test scripts through SketchCraft and check what they
// build. Needs the bridge and dev page (see docs/dev/building.md).
//   node tools/ruby-tests/run.mjs
import fs from 'node:fs';
import path from 'node:path';
import { launch, PAGE, ROOT } from '../demos/lib.mjs';

const dir = path.join(ROOT, 'tools/ruby-tests');
const b64 = (f) => fs.readFileSync(f).toString('base64');
// the House Kit as an .rbz (a zip), built here
const { execFileSync } = await import('node:child_process');
const rbz = path.join(dir, '.house_kit.rbz');
fs.rmSync(rbz, { force: true });
execFileSync('zip', ['-qr', rbz, 'house_kit.rb', 'house_kit'], { cwd: dir });

const b = await launch();
const p = await (await b.newContext()).newPage();
await p.goto(PAGE);
await p.waitForSelector('.viewport canvas');
const X = (c) => p.evaluate(async (c) => (await fetch('http://127.0.0.1:5175/exec', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(c) })).json(), c);
let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${what}`);
  if (!ok) failed++;
};

// run one menu item of an installed plugin, answering forms with their defaults
async function runItem(id, label, selection = []) {
  return p.evaluate(async ({ id, label, selection }) => {
    const { runRuby } = await import('/src/ruby.ts');
    const call = async (c) => (await fetch('http://127.0.0.1:5175/exec', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(c) })).json();
    const list = await call({ cmd: 'plugins' });
    const pl = list.plugins.find((x) => x.id === id);
    if (!pl) return { error: 'not installed' };
    const menu = (await runRuby(pl, {})).menu;
    const item = menu.indexOf(label);
    if (item < 0) return { error: `no menu item ${label} (has ${menu.join(', ')})` };
    const answers = [];
    for (let k = 0; k < 6; k++) {
      const r = await runRuby(pl, { item, answers, selection });
      if (r.need) { answers.push(r.need.defaults.map((d, i) => (r.need.lists[i] && r.need.lists[i].includes('|') ? d || r.need.lists[i].split('|')[0] : d))); continue; }
      if (r.error) return { error: r.error, menu };
      for (const cmd of r.journal) {
        const res = await call(cmd);
        if (!res.ok) return { error: `replay ${cmd.cmd}: ${res.error}`, menu };
      }
      return { ok: true, menu, out: r.out, toasts: r.toasts, steps: r.journal.length };
    }
    return { error: 'kept asking' };
  }, { id, label, selection });
}

const parts = async () => (await X({ cmd: 'parts' })).parts;
const inch = (m) => Math.round(m / 0.0254);

for (const f of ['stairs.rb', 'fence_posts.rb', 'selection.rb', 'unsupported.rb']) check((await X({ cmd: 'install_plugin', data: b64(path.join(dir, f)), name: f })).ok, `install ${f}`);
check((await X({ cmd: 'install_plugin', data: b64(rbz), name: 'house_kit.rbz' })).ok, 'install house_kit.rbz');

await X({ cmd: 'new' });
let r = await runItem('stairs', 'Stairs…');
check(r.ok, `Stairs ran (${r.error ?? r.steps + ' steps'})`);
let ps = await parts();
const steps = ps.filter((q) => q.name.startsWith('Step'));
check(steps.length === 8, `8 steps (got ${steps.length})`);
check(steps.every((q) => inch(q.size[0]) === 36 && inch(q.size[1]) === 11 && inch(q.size[2]) === 7), `each step 36 x 11 x 7 in (${JSON.stringify(steps[0]?.size.map(inch))})`);
check(steps.every((q) => q.material === 'Wood'), 'painted Wood');

await X({ cmd: 'new' });
r = await runItem('fence-posts', 'Fence posts');
check(r.ok, `Fence posts ran (${r.error ?? r.steps + ' steps'})`);
ps = await parts();
check(ps.length === 10 && ps.every((q) => q.kind === 'component'), `10 post components (got ${ps.length} ${[...new Set(ps.map((q) => q.kind))]})`);
check(ps.every((q) => inch(q.size[0]) === 36 && inch(q.size[1]) === 4), `each post 36 x 4 x 4 in (${JSON.stringify(ps[0]?.size.map(inch))})`);

await X({ cmd: 'new' });
r = await runItem('house-kit', 'Build a house');
check(r.ok, `House Kit (.rbz, extension loader) ran (${r.error ?? r.steps + ' steps'})`);
ps = await parts();
const house = ps.find((q) => q.name === 'House');
check(!!house && inch(house.size[0]) === 360 && inch(house.size[1]) === 240 && inch(house.size[2]) === 156, `a 360 x 240 in house, 156 in to the ridge (${JSON.stringify(house?.size.map(inch))})`);
check((await X({ cmd: 'info' })).materials.includes('Roof'), 'the Roof material');

const g = (await X({ cmd: 'info' })).groups.find((x) => x.name === 'House');
r = await runItem('selection', 'Paint selection red', [{ id: g.id, name: 'House', kind: 'group' }]);
check(r.ok && /Painted 1/.test(r.out) && r.toasts.includes('Painted 1'), `selection: ${r.error ?? JSON.stringify([r.out.trim(), r.toasts])}`);

r = await runItem('unsupported', 'Open a dialog');
check(!r.ok && /HtmlDialog/.test(r.error ?? ''), `unsupported API says so: ${r.error}`);

for (const id of ['stairs', 'fence-posts', 'selection', 'unsupported', 'house-kit']) await X({ cmd: 'remove_plugin', id });
fs.rmSync(rbz, { force: true });
await b.close();
console.log(failed ? `\n${failed} failed` : '\nall passed');
process.exit(failed ? 1 : 0);
