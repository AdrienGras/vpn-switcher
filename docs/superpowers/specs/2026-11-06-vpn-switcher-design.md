# vpn-switcher — design

## Intention

Proton VPN, Tailscale et NetBird cohabitent sur la même machine mais ne peuvent pas
tourner en même temps. `vpns` est une petite CLI Rust soignée pour voir l'état de chaque
VPN et basculer de l'un à l'autre en une touche.

## Objectifs

- `vpns` : sélecteur interactif en ligne (flèches + Entrée), chaque VPN avec son statut.
  À la validation : coupe tous les autres VPN actifs, puis monte le sélectionné.
- `vpns status` : tableau coloré de l'état de chaque VPN.
- `vpns up <nom>`, `vpns down <nom>`, `vpns off` (tout couper).
- Configuration TOML dans `$XDG_CONFIG_HOME/vpn-switcher/config.toml`.

## Non-objectifs

- Pas de TUI plein écran, pas de démon, pas de gestion des identifiants VPN.
- Pas de support Windows/macOS ciblé (Linux d'abord ; rien n'empêche macOS).

## Anti-objectifs

- Laisser deux VPN montés après une bascule réussie.
- Masquer une invite `sudo` ou la sortie d'erreur d'une commande qui échoue.

## Configuration

```toml
[[vpn]]
name = "proton"
up = "protonvpn connect"
down = "protonvpn disconnect"
status = "protonvpn status"
up_pattern = "Status: Connected"   # optionnel : prime sur les codes de sortie

[[vpn]]
name = "tailscale"
up = "sudo tailscale up"
down = "sudo tailscale down"
status = "tailscale status"
# up_codes = [0]     défaut
# down_codes = [1]   défaut
```

Détection d'état : si `up_pattern` est défini, état = up si le motif (regex) apparaît
dans stdout+stderr de `status`. Sinon, code dans `up_codes` (défaut `[0]`) → up, code
dans `down_codes` (défaut `[1]`) → down, autre code → inconnu. Commande `status` absente
→ état inconnu. Les commandes passent par `sh -c`.

## Approches envisagées

1. **Sélecteur en ligne (`inquire`)** — retenu : léger, joli, adapté à un choix unique.
2. TUI plein écran (`ratatui`) — trop lourd pour un choix en une touche.
3. `dialoguer` — fonctionne, rendu moins soigné.

## Décisions

- Binaire `vpns`, dépôt `vpn-switcher`, Rust édition 2024.
- Libs : `clap`, `serde` + `toml`, `directories`/`etcetera` (XDG), `inquire`,
  `owo-colors`, `indicatif`, `anyhow`/`thiserror`, `regex`.
- Les commandes up/down tournent hors du mode brut du terminal (stdin hérité) pour que
  `sudo` puisse demander son mot de passe.
- Bascule : coupe les VPN actifs ≠ cible, monte la cible, revérifie l'état.
  Cible déjà active → rien à faire. Entrée « Tout couper » dans la liste.
- Statuts vérifiés en parallèle (threads) pour un affichage rapide.
- Config absente → message clair avec le chemin et un exemple ; `vpns init` écrit un
  fichier d'exemple.
- Tests : parsing de config, détection d'état, logique de bascule (via un exécuteur
  injectable). CI GitHub Actions (fmt, clippy, test).

## Laissé au constructeur

Mise en forme exacte, messages, structure des modules.
