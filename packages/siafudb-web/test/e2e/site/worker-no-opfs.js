// Test shim: hides OPFS from the real worker so backend 'auto' must fall back to IndexedDB.
Object.defineProperty(StorageManager.prototype, 'getDirectory', { value: undefined, configurable: true });
await import('/dist/worker.js');
