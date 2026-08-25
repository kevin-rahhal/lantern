import { readFile } from "node:fs/promises";

const root = new URL("../", import.meta.url);
const pkg = JSON.parse(await readFile(new URL("package.json", root), "utf8"));
const tauri = JSON.parse(await readFile(new URL("src-tauri/tauri.conf.json", root), "utf8"));
const cargo = await readFile(new URL("src-tauri/Cargo.toml", root), "utf8");
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

const versions = new Map([
  ["package.json", pkg.version],
  ["src-tauri/tauri.conf.json", tauri.version],
  ["src-tauri/Cargo.toml", cargoVersion],
]);

const expected = pkg.version;
const mismatches = [...versions].filter(([, version]) => version !== expected);
if (mismatches.length) {
  throw new Error(`Version mismatch: ${[...versions].map(([file, version]) => `${file}=${version}`).join(", ")}`);
}

console.log(`Lantern version ${expected} is consistent.`);

