// Test shim: a device whose clock is three days ahead. Everything the real
// worker stamps (sync ops included) uses this clock.
const realNow = Date.now.bind(Date);
Date.now = () => realNow() + 3 * 24 * 60 * 60 * 1000;
await import("/dist/worker.js");
