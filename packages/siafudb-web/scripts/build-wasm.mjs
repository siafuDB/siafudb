// Builds crates/siafudb-wasm into src/wasm (wasm-bindgen --target web).
// Needs rustc with the wasm32-unknown-unknown target and wasm-bindgen-cli 0.2.130.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { rmSync, mkdirSync, statSync, readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "../../..");
const out = path.resolve(here, "../src/wasm");
const target = process.env.CARGO_TARGET_DIR ?? path.join(root, "target");
const run = (cmd, args) =>
  execFileSync(cmd, args, {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, CARGO_TARGET_DIR: target },
  });

run("cargo", [
  "build",
  "-p",
  "siafudb-wasm",
  "--target",
  "wasm32-unknown-unknown",
  "--profile",
  "wasm",
]);
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
run("wasm-bindgen", [
  "--target",
  "web",
  "--out-dir",
  out,
  "--out-name",
  "siafudb_wasm",
  "--remove-name-section",
  "--remove-producers-section",
  path.join(target, "wasm32-unknown-unknown/wasm/siafudb_wasm.wasm"),
]);
const wasm = path.join(out, "siafudb_wasm_bg.wasm");
console.log(
  `wasm: ${statSync(wasm).size} bytes raw, ${gzipSync(readFileSync(wasm)).length} bytes gzip`,
);
