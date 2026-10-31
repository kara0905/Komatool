# Contribuer à KomaTool

Les contributions sont les bienvenues. Pour une modification importante, ouvre d’abord une issue afin de discuter du besoin et de la solution.

## Préparer l’environnement

Installe Bun, Rust et les dépendances système requises par Tauri 2 pour ta plateforme, puis lance :

```sh
bun install
bun run tauri dev
```

La préparation de Waifu2x nécessite une connexion Internet au premier lancement.

## Modifier le code

- Place les traitements natifs dans le module de `src-tauri/src/processing/` correspondant à leur domaine et expose-les via `commands.rs` si l’interface doit les appeler.
- Place les éléments Svelte réutilisables dans `src/components/`.
- N’ajoute pas au dépôt les fichiers générés, les ressources Waifu2x téléchargées, des images d’utilisateurs ou des secrets.
- Garde les messages visibles par l’utilisateur en français et signale clairement les erreurs de traitement.

## Vérifier une contribution

Avant d’ouvrir une pull request, exécute :

```sh
bun run build
cd src-tauri
cargo fmt --check
cargo test
```

Décris le comportement modifié, les vérifications effectuées et joins une capture si l’interface change. Les paquets de publication sont construits séparément par plateforme sur GitHub Actions.
