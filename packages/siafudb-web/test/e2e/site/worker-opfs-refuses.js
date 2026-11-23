// Test shim: OPFS looks available (the API is there) but every call fails, as
// in Safari private browsing. Backend 'auto' must fall back to IndexedDB.
Object.defineProperty(StorageManager.prototype, "getDirectory", {
  value: () =>
    Promise.reject(
      new DOMException(
        "The operation failed for an unknown transient reason (e.g. out of memory).",
        "UnknownError",
      ),
    ),
  configurable: true,
});
await import("/dist/worker.js");
