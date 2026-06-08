import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const templatePath = "src-tauri/tauri.prod.conf.json";
const outputPath = "src-tauri/tauri.prod.generated.conf.json";

function requiredEnv(...names) {
  for (const name of names) {
    const value = process.env[name]?.trim();
    if (value) return value;
  }

  throw new Error(`Missing required environment variable: ${names.join(" or ")}`);
}

function updateEndpoint() {
  const explicitEndpoint = process.env.PIOC_UPDATE_ENDPOINT?.trim();
  if (explicitEndpoint) return explicitEndpoint;

  const githubRepository = process.env.GITHUB_REPOSITORY?.trim();
  if (githubRepository) {
    return `https://github.com/${githubRepository}/releases/latest/download/latest.json`;
  }

  throw new Error(
    "Missing update endpoint. Set PIOC_UPDATE_ENDPOINT, or run in GitHub Actions where GITHUB_REPOSITORY is available.",
  );
}

const config = JSON.parse(readFileSync(templatePath, "utf8"));
const publicKey = requiredEnv("TAURI_UPDATER_PUBLIC_KEY", "PIOC_UPDATER_PUBLIC_KEY");
const endpoint = updateEndpoint();

config.plugins ??= {};
config.plugins.updater ??= {};
config.plugins.updater.pubkey = publicKey;
config.plugins.updater.endpoints = [endpoint];

mkdirSync(dirname(outputPath), { recursive: true });
writeFileSync(outputPath, `${JSON.stringify(config, null, 2)}\n`);

console.log(`Wrote ${outputPath}`);
console.log(`Updater endpoint: ${endpoint}`);
