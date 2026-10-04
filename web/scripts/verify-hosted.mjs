import { createHash } from 'node:crypto';
import { readdir, readFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const directory = resolve(process.argv[2] ?? 'dist');
const base = new URL(process.argv[3] ?? 'https://antonsoo.github.io/logdelta/');
if (
  base.protocol !== 'https:' ||
  base.username ||
  base.password ||
  base.search ||
  base.hash ||
  !base.pathname.endsWith('/')
)
  throw new Error('Expected an HTTPS site URL ending in /, without credentials or query.');
async function files(path, prefix = '') {
  const entries = await readdir(path, { withFileTypes: true });
  const paths = [];
  for (const entry of entries) {
    if (entry.isDirectory())
      paths.push(...(await files(join(path, entry.name), `${prefix}${entry.name}/`)));
    else if (entry.isFile()) paths.push(`${prefix}${entry.name}`);
    else throw new Error(`Unexpected non-file build entry: ${entry.name}`);
  }
  return paths.sort();
}
const paths = await files(directory);
if (!paths.includes('index.html')) throw new Error('Build has no index.html.');
const modules = paths.filter((path) => /^assets\/logdelta-[A-Za-z0-9_-]+\.wasm$/.test(path));
if (modules.length !== 1 || paths.includes('logdelta.wasm'))
  throw new Error('Expected one fingerprinted engine and no fixed-name module.');
const index = await readFile(join(directory, 'index.html'), 'utf8');
if (
  !index.includes('http-equiv="Content-Security-Policy"') ||
  !index.includes("connect-src 'self'")
)
  throw new Error('Build is missing its CSP.');
const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const assets = [];
const failures = [];
for (let offset = 0; offset < paths.length; offset += 4) {
  const results = await Promise.allSettled(
    paths.slice(offset, offset + 4).map(async (path) => {
      const expected = digest(await readFile(join(directory, path)));
      const url = new URL(path.split('/').map(encodeURIComponent).join('/'), base);
      url.searchParams.set('verify', expected.slice(0, 16));
      const response = await fetch(url, { redirect: 'error', signal: AbortSignal.timeout(20_000) });
      if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
      const actual = digest(new Uint8Array(await response.arrayBuffer()));
      if (actual !== expected) throw new Error(`${path}: hosted SHA-256 differs from build`);
      return { path, sha256: actual };
    }),
  );
  for (const result of results) {
    if (result.status === 'fulfilled') assets.push(result.value);
    else failures.push(String(result.reason));
  }
}
if (failures.length) throw new Error(failures.join('\n'));
console.log(
  JSON.stringify(
    { verifiedAt: new Date().toISOString(), site: base.href, files: assets.length, assets },
    null,
    2,
  ),
);
