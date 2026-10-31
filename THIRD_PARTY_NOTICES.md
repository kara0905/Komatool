# Crédits et composants tiers

KomaTool intègre et redistribue des composants tiers dans ses paquets. Leurs licences restent distinctes de celle du code de KomaTool.

## Waifu2x NCNN Vulkan

Le script `scripts/prepare-waifu2x.mjs` télécharge la version `20250915` du binaire et des modèles depuis les [releases de nihui/waifu2x-ncnn-vulkan](https://github.com/nihui/waifu2x-ncnn-vulkan/releases). Il vérifie l’empreinte SHA-256 de l’archive avant extraction et ne conserve que les modèles CUNet et Anime Art.

Le fichier `LICENSE` fourni avec cette archive est copié dans les ressources empaquetées sous le nom `LICENSE-WAIFU2X.txt`. Le projet amont crédite également [ncnn](https://github.com/Tencent/ncnn), [libwebp](https://github.com/webmproject/libwebp), [libjpeg-turbo](https://github.com/libjpeg-turbo/libjpeg-turbo), [libpng](https://github.com/pnggroup/libpng), [zlib-ng](https://github.com/zlib-ng/zlib-ng) et [dirent](https://github.com/tronkko/dirent) comme composants utilisés. Consultez les licences et avis correspondants dans les dépôts amont.

Les poids des modèles sont redistribués depuis l’archive amont avec le moteur. Le fichier de licence du moteur ne doit pas être interprété comme une déclaration séparée sur les droits des œuvres ou données ayant servi à entraîner les modèles. Vérifiez les conditions associées aux modèles auprès de leurs auteurs avant toute redistribution commerciale ou autre usage soumis à des restrictions.

## Dépendances de l’application

Les dépendances JavaScript et Rust sont listées dans `package.json` et `src-tauri/Cargo.toml`, et leurs versions résolues sont conservées dans `bun.lock` et `src-tauri/Cargo.lock`. Chaque dépendance demeure soumise à sa propre licence. Une liste exhaustive des licences transitives devra être générée pour les distributions qui l’exigent.
