import { cpSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
cpSync(path.resolve(here, '../src/wasm'), path.resolve(here, '../dist/wasm'), { recursive: true });
