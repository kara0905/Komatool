# KomaTool

KomaTool est une application de bureau pour préparer des planches d’images et des livres numériques. Elle combine une interface Svelte avec un moteur de traitement Rust exécuté localement via Tauri.

## Fonctionnalités

- **Fusion** : assembler des images verticalement, avec réorganisation, recadrage, marges et redimensionnement.
- **Découpe** : découpe à hauteur fixe, en grille ou avec recherche heuristique des séparations ; aperçu et réglage manuel des séparateurs.
- **Fusion & redécoupe** : enchaîner ces opérations sans fichier temporaire intermédiaire.
- **Upscale** : agrandir des illustrations avec Waifu2x, ses modèles CUNet et Anime Art, et le calcul GPU ou CPU.
- **Conversion** : convertir des images en PNG, JPEG ou WebP.
- **Optimisation** : compresser sans perte ou réduire davantage le poids avec une compression JPEG réglable.
- **Export lecture** : produire une archive CBZ avec métadonnées ComicInfo, ou un PDF.
- Glisser-déposer, traitement par lots et indicateurs de progression.

L’application traite les images sur l’ordinateur de l’utilisateur. Elle ne les téléverse pas. La préparation d’un build télécharge toutefois le moteur Waifu2x depuis son dépôt amont ; le script vérifie son empreinte SHA-256 avant de l’utiliser.

## Prérequis

- [Bun](https://bun.sh/) et [Rust](https://www.rust-lang.org/tools/install).
- Les dépendances système correspondant à votre plateforme, selon le [guide de prérequis Tauri 2](https://v2.tauri.app/start/prerequisites/).
- Une connexion Internet lors de la première préparation du moteur Waifu2x.

## Développement

```sh
bun install
bun run tauri dev
```

La première commande de développement prépare Waifu2x pour la plateforme courante, puis démarre l’interface et l’application native.

## Vérifications locales

```sh
bun run build
cd src-tauri
cargo fmt --check
cargo test
```

Le benchmark manuel du découpage intelligent est ignoré pendant la suite normale. Pour le lancer :

```sh
cargo test benchmark_smart_scan -- --ignored --nocapture
```

## Création des paquets

Les paquets doivent être construits sur leur système cible. Les commandes suivantes sont utilisées par GitHub Actions :

| Plateforme | Commande | Fichiers produits |
| --- | --- | --- |
| Windows | `bun run tauri:build:windows` | Installateur NSIS `.exe` |
| Linux | `bun run tauri:build:linux` | `.deb` et `.AppImage` |
| macOS | `bun run tauri:build:macos` | `.dmg` universel Apple Silicon et Intel |

Le build Linux requiert les dépendances système Tauri/WebKitGTK listées dans le guide Tauri. Le build macOS universel compile les deux cibles Rust Apple (`aarch64-apple-darwin` et `x86_64-apple-darwin`). Les builds publics ne sont pas signés ou notariés par défaut ; les téléchargements peuvent donc présenter les avertissements habituels de SmartScreen ou Gatekeeper.

Le workflow `.github/workflows/release-packages.yml` conserve les paquets comme artefacts pour un lancement manuel. Lorsqu’un tag `vX.Y.Z` correspondant à la version de l’application est poussé, il publie les trois familles de paquets dans une GitHub Release.

## Organisation du code

- `src/App.svelte` : état de l’application, navigation entre espaces et orchestration des commandes.
- `src/components/` : composants d’interface réutilisables.
- `src-tauri/src/commands.rs` : commandes Tauri appelées depuis l’interface.
- `src-tauri/src/processing/` : traitements Rust répartis par domaine (fusion, découpe, aperçu, conversion, export et transformations).
- `scripts/prepare-waifu2x.mjs` : téléchargement contrôlé et préparation des ressources Waifu2x, exclues du dépôt.
- `.github/workflows/` : automatisation de construction des paquets.

Les versions de `package.json`, `src-tauri/tauri.conf.json` et `src-tauri/Cargo.toml` doivent rester synchronisées lors d’une publication.

## Dépendances et licences

Le code de KomaTool est distribué sous licence [MIT](LICENSE). Cette licence ne remplace pas celles des dépendances, du moteur ou des modèles tiers. Consultez [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) pour leurs crédits et conditions distinctes.
