/**
 * Which browser engine the end-to-end suites drive. Chromium unless
 * `E2E_BROWSER` says `firefox` or `webkit`; anything else is an error, so a
 * typo cannot silently fall back to Chromium.
 */
import { chromium, firefox, webkit, type BrowserType } from "playwright";

const engines: Record<string, BrowserType> = { chromium, firefox, webkit };

export const browserName = process.env.E2E_BROWSER ?? "chromium";

const chosen = engines[browserName];
if (!chosen) {
  throw new Error(
    `E2E_BROWSER must be chromium, firefox or webkit, not "${browserName}"`,
  );
}

export const browserType: BrowserType = chosen;

/**
 * Whether the engine under test has a usable OPFS. Playwright's WebKit build
 * exposes the OPFS API but refuses to open files in it, which is also what
 * Safari does in private browsing; the package then stores in IndexedDB.
 * Safari itself has a working OPFS outside private browsing.
 */
export const hasOpfs = browserName !== "webkit";

/** The storage backend `open()` is expected to pick on this engine. */
export const defaultBackend = hasOpfs ? "opfs" : "idb";
