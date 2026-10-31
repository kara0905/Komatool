import { access, chmod, copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const version = "20250915";
const platform = process.platform;
const assetByPlatform = {
  darwin: `waifu2x-ncnn-vulkan-${version}-macos.zip`,
  linux: `waifu2x-ncnn-vulkan-${version}-linux.zip`,
  win32: `waifu2x-ncnn-vulkan-${version}-windows.zip`,
};
const sha256ByAsset = {
  "waifu2x-ncnn-vulkan-20250915-linux.zip": "848e0fba55657d34da90b775b8139e9806dc754798b029f95e106ba8850a731f",
  "waifu2x-ncnn-vulkan-20250915-macos.zip": "a5b58b239eb3aa030db5464f6759637fe412d8c07891a4346ea57f708b514d42",
  "waifu2x-ncnn-vulkan-20250915-windows.zip": "7425be94b94e4c8f37a1e433ac0e0100c43790e2c37418f4b65d8235adfbdc87",
};
const asset = assetByPlatform[platform];
if (!asset) throw new Error(`Waifu2x n'est pas empaqueté pour la plateforme ${platform}.`);

const destination = join("src-tauri", "resources", "waifu2x");
const executableName = platform === "win32" ? "waifu2x-ncnn-vulkan.exe" : "waifu2x-ncnn-vulkan";
const models = ["models-cunet", "models-upconv_7_anime_style_art_rgb"];
const preparedVersion = `${version}-${platform}`;

try {
  if ((await readFile(join(destination, ".version"), "utf8")).trim() === preparedVersion) {
    await Promise.all([executableName, ...models.map((model) => `${model}/scale2.0x_model.bin`)]
      .map((path) => access(join(destination, path))));
    console.log(`Waifu2x ${version} est déjà prêt.`);
    process.exit(0);
  }
} catch { /* Le moteur n'a pas encore été préparé. */ }

const url = `https://github.com/nihui/waifu2x-ncnn-vulkan/releases/download/${version}/${asset}`;
console.log(`Téléchargement du moteur Waifu2x ${version} (${platform})…`);
const response = await fetch(url);
if (!response.ok) throw new Error(`Téléchargement Waifu2x impossible : HTTP ${response.status}`);

const temporary = await mkdtemp(join(tmpdir(), "komatool-waifu2x-"));
try {
  const archive = join(temporary, asset);
  const archiveData = new Uint8Array(await response.arrayBuffer());
  // Refuse une archive différente de celle attendue avant d'extraire ou
  // d'intégrer un exécutable dans l'application.
  const digest = createHash("sha256").update(archiveData).digest("hex");
  if (digest !== sha256ByAsset[asset]) throw new Error("L'empreinte de l'archive Waifu2x est invalide.");
  await writeFile(archive, archiveData);
  const extractor = spawnSync(platform === "win32" ? "tar" : "unzip", platform === "win32"
    ? ["-xf", archive, "-C", temporary]
    : ["-q", archive, "-d", temporary], { stdio: "inherit" });
  if (extractor.status !== 0) throw new Error("Impossible de décompresser l'archive Waifu2x.");

  const [root] = (await readdir(temporary, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .map((entry) => join(temporary, entry.name));
  if (!root) throw new Error("Archive Waifu2x vide ou invalide.");

  // Ne conserve que le binaire et les deux modèles utilisés par KomaTool.
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await writeFile(join(destination, ".gitkeep"), "");
  await copyFile(join(root, executableName), join(destination, executableName));
  for (const model of models) {
    await mkdir(join(destination, model), { recursive: true });
    for (const file of await readdir(join(root, model))) {
      await copyFile(join(root, model, file), join(destination, model, file));
    }
  }
  await copyFile(join(root, "LICENSE"), join(destination, "LICENSE-WAIFU2X.txt"));
  await writeFile(join(destination, ".version"), `${preparedVersion}\n`);
  if (platform !== "win32") await chmod(join(destination, executableName), 0o755);
  console.log(`Waifu2x prêt ; modèles intégrés : ${models.join(", ")}.`);
} finally {
  await rm(temporary, { recursive: true, force: true });
}
