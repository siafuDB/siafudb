const m = require('./pkg/wasmprobe.js');
for (const f of ['graph_roundtrip','snapshot_roundtrip','uuid_v4','systemtime','sha256_hex','siafu_execute']) {
  try { console.log(f.padEnd(20), '=>', m[f]()); } catch (e) { console.log(f.padEnd(20), '=> FAIL', String(e).split('\n')[0]); }
}
