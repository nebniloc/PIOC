import { readFileSync } from "node:fs";

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function readCargoPackageVersion(path) {
  const cargoToml = readFileSync(path, "utf8");
  const match = cargoToml.match(/^version\s*=\s*"([^"]+)"/m);

  if (!match) {
    throw new Error(`Unable to find package version in ${path}`);
  }

  return match[1];
}

const versions = {
  "package.json": readJson("package.json").version,
  "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
  "src-tauri/Cargo.toml": readCargoPackageVersion("src-tauri/Cargo.toml"),
};

const uniqueVersions = new Set(Object.values(versions));

if (uniqueVersions.size > 1) {
  console.error("PIOC version mismatch:");
  for (const [file, version] of Object.entries(versions)) {
    console.error(`  ${file}: ${version}`);
  }
  process.exit(1);
}

console.log(`PIOC version ${versions["package.json"]} is in sync.`);
