import { readdir, readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

function readOptions(args) {
  const options = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (!name?.startsWith("--") || value === undefined) {
      throw new Error(`invalid argument near ${name ?? "end of command"}`);
    }
    options.set(name.slice(2), value);
  }
  return options;
}

function required(options, name) {
  const value = options.get(name)?.trim();
  if (!value) throw new Error(`--${name} is required`);
  return value;
}

function platformKeys(assetName) {
  if (assetName.endsWith(".app.tar.gz") && assetName.includes("aarch64")) {
    return ["darwin-aarch64", "darwin-aarch64-app"];
  }
  if (
    assetName.endsWith(".app.tar.gz")
    && (assetName.includes("x64") || assetName.includes("x86_64"))
  ) {
    return ["darwin-x86_64", "darwin-x86_64-app"];
  }
  if (assetName.endsWith("-setup.exe")) {
    return ["windows-x86_64", "windows-x86_64-nsis"];
  }
  return [];
}

export function mergeUpdaterPlatforms(manifest, release, signatures, repository, version) {
  if (!manifest.platforms || typeof manifest.platforms !== "object") {
    throw new Error("updater manifest has no platforms");
  }
  if (!/^[^/\s]+\/[^/\s]+$/.test(repository)) {
    throw new Error(`invalid GitHub repository: ${repository}`);
  }
  const tag = `v${version}`;
  const assetNames = new Set(
    (release.assets ?? []).map((asset) => asset?.name).filter(Boolean),
  );

  for (const [filename, signature] of Object.entries(signatures)) {
    if (!filename.endsWith(".sig")) continue;
    const assetName = filename.slice(0, -".sig".length);
    const keys = platformKeys(assetName);
    if (keys.length === 0 || !assetNames.has(assetName)) continue;
    const url = `https://github.com/${repository}/releases/download/${tag}/${encodeURIComponent(assetName)}`;
    for (const key of keys) {
      manifest.platforms[key] = { signature: signature.trim(), url };
    }
  }

  return manifest;
}

async function readSignatures(directory) {
  const signatures = {};
  for (const filename of await readdir(directory)) {
    if (!filename.endsWith(".sig")) continue;
    signatures[filename] = await readFile(resolve(directory, filename), "utf8");
  }
  return signatures;
}

async function main() {
  const options = readOptions(process.argv.slice(2));
  const manifestPath = resolve(required(options, "manifest"));
  const releasePath = resolve(required(options, "release"));
  const signaturesDir = resolve(required(options, "signatures"));
  const repository = required(options, "repository");
  const version = required(options, "version").replace(/^v/, "");

  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`invalid semantic version: ${version}`);
  }

  const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
  const release = JSON.parse(await readFile(releasePath, "utf8"));
  const merged = mergeUpdaterPlatforms(
    manifest,
    release,
    await readSignatures(signaturesDir),
    repository,
    version,
  );
  await writeFile(manifestPath, `${JSON.stringify(merged, null, 2)}\n`);
}

if (
  process.argv[1]
  && import.meta.url === pathToFileURL(resolve(process.argv[1])).href
) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  });
}
