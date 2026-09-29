import { readFileSync } from 'node:fs';

const tag = process.argv[2];
const app = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
const frontend = JSON.parse(readFileSync('package.json', 'utf8'));
const manifest = readFileSync('src-tauri/Cargo.toml', 'utf8');
const cargoVersion = manifest.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const expectedTag = `v${app.version}`;

if (!/^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(tag || '')) {
  throw new Error(`发布标签须采用 v<版本号>，收到：${tag || '空'}`);
}
if (tag !== expectedTag || frontend.version !== app.version || cargoVersion !== app.version) {
  throw new Error(
    `版本不一致：标签=${tag}，Tauri=${app.version}，package.json=${frontend.version}，Cargo.toml=${cargoVersion}`,
  );
}
console.log(`发布版本已确认：${tag}`);
