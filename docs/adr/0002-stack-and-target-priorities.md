# ADR-0002: Stack and target priorities

- Status: **Accepted** (owner decision, 2026-11-13)

## Decisions

1. **Languages we maintain: Rust, TypeScript, Astro** (plus Swift and Kotlin
   only as generated bindings). No React Native / Expo native module: it would
   add a language and a runtime to maintain.
2. **Web first.** The product surface is mostly PWAs while the native apps are
   in development, so the browser build (wasm + TypeScript API + OPFS /
   IndexedDB persistence + WebCrypto key handling) is the first deliverable.
3. **Native wrapping = UniFFI to Swift and Kotlin**, generated from the same
   Rust core. Nothing hand-written beyond packaging.
4. **Node 24 or newer** for all JS tooling (`.nvmrc`, `engines`, CI). Older
   Node versions are unsupported.
5. Server/relay: Rust. Docs: Mintlify (`siafudb-docs`). Marketing/demo sites:
   Astro.

## Phases

| Phase | Deliverable | Gate |
| --- | --- | --- |
| 0 | Engine fork pinned; ADRs; Node 24 baseline | this PR |
| 1 | `@siafudb/web` (wasm + TS + persistence + encryption); `siafudb-store` (page store trait + encryption); `gsp-wire` (signed op envelope); `siafudb-ffi` (Swift/Kotlin); Astro PWA demo; CI | tests green on Node 24 + headless Chromium |
| 2 | Relay + sync in browser (GSP v0, Personal fragment); service-worker offline | two browsers converge via a relay that sees only ciphertext |
| 3 | iOS/Android apps on the FFI; page-level storage inside the engine if memory-bound | real-device smoke tests |
