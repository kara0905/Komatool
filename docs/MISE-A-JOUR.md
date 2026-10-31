# Publication des mises à jour KomaTool

KomaTool vérifie les nouvelles versions au démarrage. Une mise à jour disponible
est proposée dans l'application et peut être installée en un clic. Les paquets
sont signés : l'application vérifie leur signature avant installation.

## Protéger la clé de signature

La clé publique est intégrée à `src-tauri/tauri.conf.json`. La clé privée locale
se trouve dans `src-tauri/komatool-updater.key` ; elle est ignorée par Git et ne
doit jamais être commitée ni publiée. Fais-en une sauvegarde chiffrée : perdre
cette clé empêchera les installations existantes de vérifier les prochaines
mises à jour.

Dans GitHub, ouvre **Settings → Secrets and variables → Actions → New repository
secret** et crée le secret `TAURI_SIGNING_PRIVATE_KEY` avec le contenu complet de
`src-tauri/komatool-updater.key`. La clé générée n'a pas de phrase secrète ; aucun
secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` n'est nécessaire.

## Publier une version

1. Incrémente la même version dans `package.json` et `src-tauri/tauri.conf.json`.
2. Crée et pousse un tag Git au format `vX.Y.Z` correspondant à ces versions.
3. GitHub Actions construit les installateurs Windows, Linux et macOS, leurs
   signatures, ainsi que `latest.json`, puis les attache à la release GitHub.

Le flux de mise à jour est prêt dès qu'un premier paquet signé est publié. Les
versions déjà installées avec un paquet non signé par cette clé devront être
réinstallées manuellement une première fois.
