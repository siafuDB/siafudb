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
