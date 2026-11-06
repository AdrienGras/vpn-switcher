# vpn-switcher 0.3.0 — sortie publique : design

## Intention

`vpns` fonctionne en v0.2.0 sur le laptop d'Adrien. Le but de la 0.3.0 est d'en faire
un produit public bien fini : qu'un inconnu le découvre, l'installe en une commande et le
configure sans lire tout le README.

**Public visé :** les gens comme Adrien — consultants, freelances, salariés en temps
partagé — qui passent d'un client ou d'une entreprise à l'autre et accumulent des VPN qui
ne cohabitent pas (VPN d'entreprise OpenVPN/WireGuard/AnyConnect/Fortinet, plus
Tailscale, NetBird, Proton…). Ils sont sur Linux ou macOS.

## Objectifs

Trois lots, livrés ensemble en **0.3.0**, dans cet ordre (les fonctionnalités d'abord, pour
que le README et le GIF les montrent) :

1. **Configuration facile :** `vpns init` détecte les VPN installés et leurs profils ;
   `vpns config path|edit|check` ; complétions shell et page man.
2. **Distribution et CI :** publication sur crates.io (`cargo install vpn-switcher`) et
   cargo-binstall (`cargo binstall vpn-switcher`), binaires Linux + macOS construits et
   publiés par la CI avec cache et attestations, MSRV réelle vérifiée en CI.
3. **Image publique :** README attrayant (GIF de démo, badges, Why, presets, FAQ),
   hygiène minimale du dépôt.

## Non-objectifs

- **Windows** : hors périmètre (le code repose sur `sh -c`, les groupes de processus et
  les signaux Unix ; les clients VPN d'entreprise Windows ont rarement une CLI). Le README
  le dit.
- Paquets système (AUR, Homebrew, Nix, .deb/.rpm) : plus tard, s'il y a des utilisateurs.
  Pour l'instant, distribution via cargo et les archives GitHub uniquement.
- cargo-dist, release-plz.
- JSON Schema de la config.
- Complétion dynamique des noms de VPN (API `unstable-dynamic` de clap_complete).
- Cibles linux-gnu (le binaire musl statique tourne partout ; binstall le choisit
  automatiquement sur un hôte gnu).
- CONTRIBUTING, modèles d'issues et de PR.
- Signature des binaires au-delà des attestations de provenance.

## Anti-objectifs (échec même si « ça marche »)

- `vpns init` écrase une config existante sans `--force`.
- La détection modifie quoi que ce soit : elle ne lance **jamais** de commande up/down, ni
  `sudo`, ni rien qui change l'état réseau. Lecture seule, bornée dans le temps.
- Un preset non vérifié présenté comme fiable.
- Le README cassé sur crates.io (image en chemin relatif, liens relatifs morts).
- Casser la compatibilité binstall avec les archives déjà publiées (v0.2.0).
- Un CHANGELOG généré automatiquement : il reste rédigé à la main.
- Une API de lib qui devient de fait un engagement semver.

## Approches envisagées (chaîne de release)

| Approche | Apport | Coût | Décision |
|---|---|---|---|
| **Workflow maison étoffé** | Garde le CHANGELOG en prose, le format d'archive (déjà compatible binstall), le contrôle total. | Quelques jobs à écrire. | **Retenue** par Adrien. |
| cargo-dist | Installeur shell, Homebrew, artefacts binstall, checksums. | Reprend la main sur `release.yml`, renomme les artefacts ; Homebrew hors périmètre ; l'installeur shell n'apporte rien de plus que binstall pour ce public. | Écartée. |
| release-plz | Bump de version, PR de release, publication crates.io. | Génère le changelog depuis les commits, contre la prose soignée actuelle. | Écartée. |

## Lot 1 — Configuration

### `vpns init` : détection par défaut

Comportement (validé par Adrien sur la maquette ci-dessous) :

```
$ vpns init
Found 5 VPN profiles:
› [x] tailscale        tailscale
  [x] netbird          netbird
  [x] acme             NetworkManager (openvpn)
  [x] globex-wg        NetworkManager (wireguard)
  [x] proton           protonvpn
[space toggle · enter write · esc cancel]
Wrote ~/.config/vpn-switcher/config.toml (5 VPNs). Check it with `vpns config check`.
```

- `vpns init` lance les détecteurs, affiche une liste à cocher (inquire `MultiSelect`),
  puis écrit les profils choisis. Tout est coché par défaut. *Choix de Claude :* un
  preset non vérifié porte une marque visible dans la liste (ex. `acme   NetworkManager
  (openvpn) · unverified`), pas seulement le commentaire dans le TOML.
- **Fichier existant :** les profils choisis sont **ajoutés en fin de fichier** (texte
  ajouté tel quel, pour préserver les commentaires de l'utilisateur) ; un profil dont le
  `name` existe déjà dans le fichier n'est pas proposé (ou est marqué « déjà configuré »
  et désactivé). Rien n'est écrasé.
- **`--force`** : remplace le fichier par la sélection.
- **Pas de terminal (stdin/stdout non TTY), ou rien de détecté :** écrit l'exemple commenté
  actuel (`config::EXAMPLE`), avec les mêmes règles d'existence qu'aujourd'hui (erreur si le
  fichier existe, sauf `--force`). Si rien n'est détecté, le message le dit.
- **`--example`** : force l'écriture de l'exemple commenté sans détection (utile en script
  et dans les tests).
- Esc / Ctrl-C dans la liste : annule sans rien écrire, comme le sélecteur actuel (qui
  affiche `Cancelled.` et sort en **0** ; correction : une version antérieure de ce
  document disait 130 par erreur).
- Après écriture : message invitant à lancer `vpns config check`.

Le bloc TOML écrit pour chaque profil porte une ligne de commentaire indiquant sa source
(`# detected: NetworkManager (openvpn)`) et, pour un preset non vérifié,
`# preset not verified on a real VPN — run \`vpns config check\``.

### Détecteurs

La détection est **en lecture seule** : présence d'un binaire dans le `PATH`, lecture de
répertoires, commandes de listage. Les commandes de listage passent par le runner existant
(délai, groupe de processus) ; un listage qui échoue ou expire ne fait que retirer ce
détecteur, sans erreur fatale.

« Vérifié » = sorties et codes relevés sur une vraie machine. Les autres presets viennent
de la documentation officielle des outils et sont marqués non vérifiés.

| Détecteur | OS | Détection | Profils | Preset (`up` / `down` / `status`) | Lecture du statut | Vérifié |
|---|---|---|---|---|---|---|
| Proton VPN | Linux | `protonvpn` dans le PATH | un | `protonvpn connect` / `protonvpn disconnect` / `protonvpn status` | `up_pattern = "Status: Connected"` | oui |
| Tailscale | Linux, macOS | `tailscale` dans le PATH (macOS : aussi `/Applications/Tailscale.app/Contents/MacOS/Tailscale`) | un | `tailscale up` / `tailscale down` / `tailscale status` | codes par défaut (0 up, 1 down) | oui (Linux) |
| NetBird | Linux, macOS | `netbird` dans le PATH | un | `netbird up` / `netbird down` / `netbird status` | `up_pattern = "Management: Connected"` | oui (Linux) |
| NetworkManager | Linux | `nmcli -t -f NAME,TYPE connection show`, types `vpn` et `wireguard` | **un par connexion** | `nmcli connection up id '<n>'` / `nmcli connection down id '<n>'` / `nmcli -g GENERAL.STATE connection show id '<n>'` | `up_pattern = "^activated$"` | partiel : profil inactif relevé (sortie vide, exit 0) ; actif non relevé → marqué non vérifié |
| wg-quick | Linux | `wg-quick` dans le PATH | un par `/etc/wireguard/*.conf` si le dossier est lisible ; sinon **un modèle** à compléter (le dossier est en général `root` 700) | `sudo wg-quick up <n>` / `sudo wg-quick down <n>` / `ip link show <n>` | codes par défaut (`ip link show` : 0 si l'interface existe, 1 sinon — relevé) | non |
| OpenVPN (systemd) | Linux | `openvpn` dans le PATH | un par `/etc/openvpn/client/*.conf` lisible ; sinon un modèle | `sudo systemctl start openvpn-client@<n>` / `sudo systemctl stop …` / `systemctl is-active --quiet openvpn-client@<n>` | `down_codes = [3]` (relevé : 3 = inactif) | non |
| OpenConnect (AnyConnect, GlobalProtect) | Linux, macOS | `openconnect` dans le PATH | **un modèle** (l'adresse du serveur est propre à chaque client) | à définir par le constructeur, contraintes ci-dessous | — | non |
| openfortivpn (Fortinet) | Linux, macOS | `openfortivpn` dans le PATH | un modèle | à définir, contraintes ci-dessous | — | non |
| Mullvad | Linux, macOS | `mullvad` dans le PATH | un | `mullvad connect` / `mullvad disconnect` / `mullvad status` | `up_pattern = "^Connected"` | non |
| Cloudflare WARP | Linux, macOS | `warp-cli` dans le PATH | un | `warp-cli connect` / `warp-cli disconnect` / `warp-cli status` | motif « Connected » à confirmer dans la doc | non |
| VPN système macOS | macOS | `scutil --nc list` | un par service | `scutil --nc start '<n>'` / `scutil --nc stop '<n>'` / `scutil --nc status '<n>'` | `up_pattern = "^Connected"` | non |

Contraintes communes aux presets :
- Une commande `status` tourne **sans terminal** : elle ne doit jamais demander de mot de
  passe (pas de `sudo` dans `status`). Les `up`/`down` peuvent utiliser `sudo`.
- Les noms de profils injectés dans une commande sont **quotés pour `sh`** (apostrophes
  simples, échappement des apostrophes internes). Une connexion NetworkManager peut
  s'appeler `ACME (Paris) — l'agence`.
- Le `name` côté vpns est dérivé du nom du profil : utilisable en ligne de commande (pas
  d'espace ; règle exacte au choix du constructeur), unique dans le fichier (suffixe en cas
  de collision, ex. un profil NetworkManager appelé `tailscale`).
- Modèles OpenConnect / openfortivpn : un bloc **commenté** (pour ne pas casser le
  chargement de la config tant que le serveur n'est pas renseigné), avec un placeholder de
  serveur clair. Pour un démon en arrière-plan, le statut doit pouvoir se lire sans root
  (ex. présence de l'interface `tun`, ou `pgrep`), le constructeur choisit et documente.
- Les motifs « vérifiés » existants (Proton, Tailscale, NetBird) sont repris tels quels de
  `config::EXAMPLE`.

Organisation du code : un nouveau module `detect` (table de détecteurs, chacun produisant
des presets `{ name, source, toml, verified }`). Le rendu TOML et la dérivation des noms
sont des fonctions pures testées unitairement ; la détection est testée avec de faux
binaires placés dans un `PATH` temporaire (faux `nmcli` qui imprime une liste, etc.),
jamais avec les vrais outils. Les détecteurs propres à un OS sont filtrés par
`cfg(target_os)` ou à l'exécution, au choix du constructeur.

### `vpns config`

- **`vpns config path`** : affiche le chemin effectivement utilisé (`-c`, puis
  `VPNS_CONFIG`, puis le chemin XDG) et indique s'il existe.
- **`vpns config edit`** : ouvre le fichier dans `$VISUAL`, sinon `$EDITOR`, sinon `vi`
  (la variable peut contenir des arguments, ex. `code --wait` : passer par `sh -c`). Si le
  fichier n'existe pas : erreur qui suggère `vpns init`. À la fermeture de l'éditeur,
  lance `check` et sort avec son code.
- **`vpns config check`** :
  1. charge et valide le fichier (erreurs existantes : syntaxe, nom vide, doublon, regex) ;
  2. lance toutes les commandes `status` en parallèle (sondes existantes, lecture seule) ;
  3. affiche une ligne par VPN : nom, état, raison (mêmes raisons que `-v`).

```
$ vpns config check
✓ ~/.config/vpn-switcher/config.toml: 4 VPNs, valid
  tailscale   up       exit code 0
  netbird     down     output does not match "Management: Connected"
  acme        down     exit code 1
  globex-wg   unknown  command not found: wg-quick (exit 127)
1 VPN cannot be read: its status command is probably wrong.
```

  Code de sortie : 0 si le fichier est valide et que chaque VPN **ayant** une commande
  `status` est lisible (up ou down) ; 1 sinon. Un VPN sans `status` est affiché
  `unknown  no status command` et **ne fait pas échouer** le check (c'est un choix de config
  légitime).

### Complétions et page man

- `vpns completions <bash|zsh|fish|elvish|powershell>` (clap_complete), complétions
  statiques.
- Page man générée par clap_mangen. *Choix du constructeur :* comment l'exposer (sous-
  commande cachée `vpns man`, ou `build.rs`/xtask) ; contrainte : la CI doit pouvoir la
  produire pour chaque archive, y compris la cible macOS x86_64 compilée en cross (le
  binaire cross ne s'exécute pas forcément sur le runner : générer depuis un binaire hôte).
- Les archives de release contiennent `completions/` (bash, zsh, fish) et `vpns.1`.
- Le README documente l'installation des complétions pour bash, zsh et fish.

## Lot 2 — Distribution et CI

### Cargo.toml et crate

- `rust-version = "1.87"` : plancher mesuré (fixé par `etcetera 0.11`, les autres
  dépendances demandent ≤ 1.85, le code n'utilise rien de plus récent). À revérifier par le
  job MSRV ; si une nouvelle dépendance (clap_complete, clap_mangen) le relève, ajuster et
  le dire.
- `version = "0.3.0"`.
- Lib conservée mais cachée : modules `#[doc(hidden)]`, doc de crate disant que la lib est
  un détail d'implémentation du binaire, sans garantie semver, et renvoyant au README.
- `include` (ou `exclude`) pour que le paquet crates.io ne contienne que `src/`, `tests/`,
  `Cargo.*`, `README.md`, `LICENSE`, `CHANGELOG.md` (pas `docs/`, `.github/`, `demo/`,
  ni le GIF). Vérifier avec `cargo package --list`.
- Métadonnées binstall, compatibles avec les archives déjà publiées :

```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/vpns-{ target }.tar.gz"
bin-dir = "vpns-{ target }/{ bin }{ binary-ext }"
pkg-fmt = "tgz"
```

  À tester localement (`cargo-binstall` est installé) contre la release v0.2.0 existante
  avant la première publication, sans installer sur le système (dry-run ou `--root`
  temporaire).

### Cibles

| Cible | Runner | Tests |
|---|---|---|
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | oui |
| `aarch64-unknown-linux-musl` | `ubuntu-24.04-arm` | oui |
| `aarch64-apple-darwin` | `macos-latest` | oui |
| `x86_64-apple-darwin` | `macos-latest` (cross) | non (binaire cross) |

Format d'archive inchangé : `vpns-<target>.tar.gz` contenant `vpns-<target>/` (binaire
`vpns`, README, LICENSE, CHANGELOG, `completions/`, `vpns.1`), plus `.sha256` (sur macOS,
`shasum -a 256` produit le même format). Ajouter `completions/` et `vpns.1` dans ce dossier
reste compatible : seul `bin-dir` de binstall et la commande d'install du README dépendent
du chemin du binaire, qui ne change pas. Noms des complétions : ceux de clap_complete par
défaut (`vpns.bash`, `_vpns`, `vpns.fish`).

### `release.yml` (sur tag `v*`)

1. **Vérification :** le tag (`v` retiré) est égal à la version de `Cargo.toml` ; la section
   du CHANGELOG n'est pas vide. Échec sinon, avant toute création.
2. **draft :** inchangé (release brouillon depuis le CHANGELOG, `--verify-tag`).
3. **build :** matrice ci-dessus, `Swatinem/rust-cache@v2` avec une clé par cible, tests
   sur les cibles natives, build release `--locked`, packaging, **attestation de
   provenance** (`actions/attest-build-provenance`) sur chaque archive, upload `--clobber`.
4. **publish :** passe la release en publique et `--latest` (inchangé).
5. **crates-io :** après `publish` (pour que binstall trouve les archives dès que la
   version apparaît sur crates.io), `cargo publish --locked` avec **trusted publishing**
   (`rust-lang/crates-io-auth-action`, `id-token: write`). Si ce job échoue, il est
   relançable seul. *Choix de Claude :*
   - le job ne tourne que si la variable de dépôt `CRATES_IO_TRUSTED_PUBLISHING` vaut
     `true` ; sinon il est sauté avec une notice. Cela couvre la toute première
     publication, faite à la main (voir « Actions manuelles ») ;
   - si la version existe déjà sur crates.io, le job réussit sans rien faire (relances).

Permissions déclarées **par job**, au plus juste (`contents: write` pour la release,
`id-token: write` + `attestations: write` pour l'attestation, `id-token: write` pour
crates.io).

Procédure de sortie pour Adrien (à écrire dans un `RELEASING.md` court ou en tête du
CHANGELOG, au choix du constructeur) : mettre la version dans `Cargo.toml`, écrire la
section du CHANGELOG, commiter, taguer `vX.Y.Z`, pousser le tag. Le reste est automatique.

### `ci.yml` (push sur main, PR)

- `check` : fmt + clippy `-D warnings` (Linux).
- `test` : matrice `ubuntu-latest` + `macos-latest`, `cargo test --locked`, rust-cache.
- `msrv` : toolchain 1.87, `cargo test --locked` (ou au minimum `cargo check
  --all-targets --locked` si une dépendance de test l'empêche — à constater et documenter).
- `deny` : `cargo-deny` (advisories, licences, bans, sources) avec un `deny.toml`.
- Les tests existants doivent passer sur macOS ; toute adaptation nécessaire (sémantique de
  `sh`, codes de sortie, signaux) est corrigée dans le code ou les tests, pas contournée.

### Dépôt

- `.github/dependabot.yml` : `cargo` et `github-actions`, hebdomadaire, mises à jour
  groupées.
- `SECURITY.md` : signalement via le « private vulnerability reporting » de GitHub.

### Actions manuelles d'Adrien (le constructeur les liste, ne peut pas les faire)

*Choix de Claude :* ordre de la première sortie.

1. Pousser le tag `v0.3.0`. La release GitHub devient publique avec ses archives, et le
   job `crates-io` est sauté (variable absente).
2. Première publication à la main, **depuis le commit tagué** (`git checkout v0.3.0`) :
   `cargo login` avec un token crates.io, puis `cargo publish --locked`. crates.io ne permet
   pas de déclarer un éditeur de confiance avant que le crate existe. Comme les archives sont
   déjà publiques, binstall fonctionne dès que la version apparaît.
3. Déclarer le trusted publisher sur crates.io : dépôt `AdrienGras/vpn-switcher`, workflow
   `release.yml` (environnement GitHub optionnel). Puis créer la variable de dépôt
   `CRATES_IO_TRUSTED_PUBLISHING=true`.
4. Révoquer le token crates.io.
5. Activer le private vulnerability reporting dans les réglages du dépôt.

Le constructeur fournit ces étapes pas à pas au moment voulu.

## Lot 3 — README et image publique

### GIF de démo

- `demo/demo.tape` (vhs) + `demo/config.toml` décrivant de faux VPN aux noms crédibles de
  clients fictifs (`acme`, `globex`, plus `tailscale`), pilotés par des fichiers témoins
  dans un dossier temporaire (`touch`/`rm`/`test -f`, avec de courtes pauses pour le
  réalisme). Aucun vrai VPN.
- Scénario : `vpns status`, bascule via le sélecteur, `vpns config check`. Court (< 20 s).
- Workflow `demo.yml` en `workflow_dispatch` : construit `vpns`, rejoue la tape, ouvre une
  PR avec le GIF régénéré. Pas de rendu à chaque push.
- *Choix de Claude :* premier rendu. Un workflow `workflow_dispatch` ne peut être lancé
  qu'une fois présent sur `main`. L'ordre est donc :
  1. merger le travail 0.3.0 ;
  2. lancer `demo.yml` ;
  3. merger la PR du GIF ;
  4. **seulement ensuite**, taguer `v0.3.0`. Le README publié sur crates.io est figé au
     moment du publish, il doit donc pointer vers un GIF qui existe déjà.

  Vhs n'est pas nécessaire sur la machine d'Adrien.
- Dans le README, le GIF est référencé par **URL absolue**
  (`https://raw.githubusercontent.com/AdrienGras/vpn-switcher/main/demo/demo.gif`) pour
  s'afficher aussi sur crates.io.

### Plan du README (anglais)

1. Titre, une phrase d'accroche, GIF.
2. Badges : version crates.io, téléchargements, CI, MSRV, licence.
3. **Why** : le cas du consultant qui change de client et accumule des VPN incompatibles.
4. **Install** : `cargo binstall vpn-switcher`, `cargo install vpn-switcher` (Rust 1.87+),
   archives GitHub (commande actuelle, étendue à macOS), vérification de provenance
   (`gh attestation verify`). Plateformes : Linux, macOS ; Windows non supporté.
5. **Quick start** : `vpns init` (détection) → `vpns config check` → `vpns`.
6. **Usage** : section actuelle, complétée par `config` et `completions`.
7. **Presets** : un bloc prêt à copier par fournisseur du tableau des détecteurs, chacun
   marqué vérifié ou non.
8. **Configuration** : référence actuelle (clés, lecture du statut).
9. **FAQ** : sudo et mot de passe, operator Tailscale, VPN sans commande de statut, codes
   de sortie (0 / 1 / 130), un même outil avec plusieurs profils.
10. Licence.

Tous les liens internes au dépôt en URL absolue (rendu crates.io).

## Organisation du travail

- Le travail se fait sur une branche `release-0.3.0`. Adrien a accepté qu'elle soit poussée
  tôt, avec une PR en brouillon sur le dépôt public.
- **La première tâche met en place `ci.yml`, avec la matrice macOS et le job MSRV.** Ainsi,
  les écarts entre macOS et Linux apparaissent avant que le reste soit construit. Il n'y a
  pas de Mac sur la machine de développement : la CI est le seul retour sur macOS.
- Ordre de fin :
  1. merge de la PR ;
  2. rendu du GIF et merge de la PR du GIF ;
  3. tag `v0.3.0` ;
  4. actions manuelles d'Adrien.

## Ce qui est laissé au constructeur

- Règle exacte de dérivation des noms de VPN et de suffixe en cas de collision.
- Exposition de la page man (sous-commande cachée ou génération au build).
- Presets OpenConnect / openfortivpn (forme exacte des commandes, lecture du statut sans
  root), dans le respect des contraintes ci-dessus.
- Filtrage des détecteurs par OS (compilation ou exécution).
- Emplacement de la procédure de release (`RELEASING.md` ou autre).
- Contenu du `deny.toml` (licences acceptées selon l'arbre de dépendances réel).
- Ordre et découpage des commits ; mise à jour du CHANGELOG (section 0.3.0, rédigée à la
  main, en anglais, dans le style existant).

## Contraintes du projet (rappel)

- Rust édition 2024, clippy `-D warnings`, tests existants (59) toujours verts.
- README et sorties de la CLI en anglais.
- Ne jamais lancer up/down sur les vrais VPN de la machine ; tests avec de faux VPN.
- Pas de `curl` en pipe sans `rtk proxy` sur la machine d'Adrien (hook rtk).
