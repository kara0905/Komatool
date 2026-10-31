import { cp, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";

const [assetsDirectory, tag, repository] = process.argv.slice(2);

if (!assetsDirectory || !tag || !repository) {
  throw new Error(
    "Usage : node scripts/create-updater-manifest.mjs <dossier-assets> <tag> <propriétaire/dépôt>",
  );
}

async function listFiles(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(
    entries.map((entry) => {
      const entryPath = path.join(directory, entry.name);
      return entry.isDirectory() ? listFiles(entryPath) : [entryPath];
    }),
  );
  return nested.flat();
}

function findAsset(files, predicate, description) {
  const asset = files.find((file) => predicate(path.basename(file)));
  if (!asset) throw new Error(`Artefact de mise à jour manquant : ${description}`);
  return asset;
}

const version = tag.replace(/^v/, "");
const outputDirectory = path.join(assetsDirectory, "publish");
await mkdir(outputDirectory, { recursive: true });

const files = await listFiles(assetsDirectory);
const windowsInstaller = findAsset(files, (name) => name.endsWith(".exe"), "installateur Windows");
const windowsSignature = findAsset(files, (name) => name === `${path.basename(windowsInstaller)}.sig`, "signature Windows");
const linuxAppImage = findAsset(files, (name) => name.endsWith(".AppImage"), "AppImage Linux");
const linuxSignature = findAsset(files, (name) => name === `${path.basename(linuxAppImage)}.sig`, "signature Linux");
const macArchive = findAsset(files, (name) => name.endsWith(".app.tar.gz"), "archive macOS pour la mise à jour");
const macSignature = findAsset(files, (name) => name === `${path.basename(macArchive)}.sig`, "signature macOS");

// Aplatit les artefacts : les actifs d'une release GitHub sont référencés par nom de fichier.
const releaseAssets = files.filter((file) =>
  /\.(exe|deb|AppImage|dmg|app\.tar\.gz)(\.sig)?$/i.test(path.basename(file)),
);
for (const asset of releaseAssets) {
  await cp(asset, path.join(outputDirectory, path.basename(asset)));
}

const releaseUrl = (file) =>
  `https://github.com/${repository}/releases/download/${tag}/${path.basename(file)}`;
const signature = async (file) => readFile(file, "utf8");

const manifest = {
  version,
  notes: `KomaTool ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: await signature(windowsSignature),
      url: releaseUrl(windowsInstaller),
    },
    "linux-x86_64": {
      signature: await signature(linuxSignature),
      url: releaseUrl(linuxAppImage),
    },
    "darwin-x86_64": {
      signature: await signature(macSignature),
      url: releaseUrl(macArchive),
    },
    "darwin-aarch64": {
      signature: await signature(macSignature),
      url: releaseUrl(macArchive),
    },
  },
};

await writeFile(path.join(outputDirectory, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
