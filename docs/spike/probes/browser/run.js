// node run.js  -- serves web/ and drives headless Chromium via Playwright.
const http = require('http'), fs = require('fs'), path = require('path'), os = require('os');
const { chromium } = require('playwright');
const root = path.join(__dirname, 'web');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm' };
const srv = http.createServer((q, r) => {
  const f = path.join(root, decodeURIComponent(q.url.split('?')[0]).replace(/^\/$/, '/index.html'));
  if (!f.startsWith(root) || !fs.existsSync(f)) { r.writeHead(404); return r.end(); }
  r.writeHead(200, { 'content-type': mime[path.extname(f)] || 'application/octet-stream' });
  fs.createReadStream(f).pipe(r);
});
const res = {};
const log = (k, v) => { res[k] = v; console.log(k, JSON.stringify(v)); };
(async () => {
  await new Promise(r => srv.listen(0, '127.0.0.1', r));
  const url = `http://127.0.0.1:${srv.address().port}/`; // localhost-equivalent => secure context
  const browser = await chromium.launch();
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  page.on('pageerror', e => console.log('PAGEERROR', e.message));
  page.on('console', m => m.type() === 'error' && console.log('CONSOLE', m.text()));
  const load = async (p) => { await p.goto(url); await p.waitForFunction('window.ready'); };
  await load(page);
  log('1a_main', await page.evaluate(() => mainSmoke()));
  log('1b_worker', await page.evaluate(() => rpc('smoke')));
  log('4_storage', await page.evaluate(() => storageInfo()));
  for (const backend of ['opfs', 'idb']) {
    log(`${backend}_persist`, await page.evaluate((b) => rpc('persist', { backend: b }), backend));
    await page.reload(); await page.waitForFunction('window.ready');
    log(`${backend}_restore_after_reload`, await page.evaluate((b) => rpc('restore', { backend: b }), backend));
    log(`${backend}_tamper`, await page.evaluate((b) => rpc('tamper', { backend: b }), backend));
  }
  // fresh page, same context
  const p2 = await ctx.newPage(); await load(p2);
  log('opfs_restore_newpage', await p2.evaluate(() => rpc('restore', { backend: 'opfs' })));
  log('5_cdc', await p2.evaluate(() => rpc('cdc')).catch(e => ({ error: e.message })));
  log('6_bench', await p2.evaluate(() => rpc('bench', { n: 10000 })));
  await browser.close();
  // persistent profile: survives browser process restart
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pw-prof-'));
  let c = await chromium.launchPersistentContext(dir); let p = await c.newPage(); await load(p);
  log('pp_storage', await p.evaluate(() => storageInfo()));
  log('pp_persist', await p.evaluate(() => rpc('persist', { backend: 'opfs' })));
  await c.close();
  c = await chromium.launchPersistentContext(dir); p = await c.newPage(); await load(p);
  log('pp_restore_after_browser_restart', await p.evaluate(() => rpc('restore', { backend: 'opfs' })));
  await c.close();
  srv.close();
  fs.writeFileSync(path.join(__dirname, 'results.json'), JSON.stringify(res, null, 1));
})().catch(e => { console.error('FATAL', e); process.exit(1); });
