# Task 01: Project Scaffolding

## Context
Point de depart du projet Vortex. Initialiser la structure Tauri 2 + Vite + React avec tous les outils du stack definis dans ARCHI.md. Ce scaffolding pose les fondations sur lesquelles toutes les autres taches s'appuient.

## Scope
- Initialiser un projet Tauri 2 avec Vite + React 19 + TypeScript
- Configurer Tailwind CSS 4 + shadcn/ui
- Configurer oxlint + oxfmt (pas d'ESLint/Prettier)
- Configurer Vitest + Testing Library
- Configurer cargo test + cargo-llvm-cov
- Creer le Nix flake pour l'environnement de dev reproductible
- Structure de dossiers hexagonale cote Rust

## Implementation Details

### Files to Create/Modify

**Racine :**
- `package.json` — React 19, Vite, TypeScript, Tailwind, dependencies frontend
- `vite.config.ts` — Config Vite pour Tauri
- `tsconfig.json` — Strict mode TypeScript
- `tailwind.config.ts` — Design tokens (colors, fonts IBM Plex Mono + Inter)
- `flake.nix` — Nix devshell (Rust toolchain, node, wasm target, system libs)
- `flake.lock`
- `.gitignore`
- `LICENSE` — GPL-3.0

**`src-tauri/` :**
- `Cargo.toml` — Tauri 2, tokio, serde, thiserror, tracing
- `tauri.conf.json` — Identifiant app, window config, permissions
- `build.rs`
- `src/main.rs` — Entry point
- `src/lib.rs` — Composition root (vide pour l'instant, structure preparee)
- `src/domain/mod.rs` — Module domaine (vide)
- `src/application/mod.rs` — Module application (vide)
- `src/adapters/mod.rs` — Module adapters (vide)
- `src/adapters/driving/mod.rs`
- `src/adapters/driven/mod.rs`

**`src/` (frontend) :**
- `src/main.tsx` — React entry
- `src/App.tsx` — Router placeholder
- `src/components/ui/` — shadcn/ui init (button, badge au minimum)
- `src/index.css` — Tailwind directives + design tokens CSS custom properties

**Config :**
- `.oxlintrc.json` — Config oxlint
- `vitest.config.ts` — Config Vitest avec jsdom

### Key Functionality
- `npm run dev` lance Vite en mode Tauri dev
- `cargo test` passe (meme si 0 tests)
- `npx vitest run` passe (meme si 0 tests)
- `cargo clippy -- -D warnings` passe sans warning
- `npx oxlint .` passe sans erreur
- `nix develop` ouvre un shell avec tous les outils disponibles

### Technologies Used
- Tauri 2.x (npm create tauri-app)
- Vite 6.x
- React 19
- TypeScript 5.x strict
- Tailwind CSS 4
- shadcn/ui (npx shadcn@latest init)
- oxlint + oxfmt
- Vitest + @testing-library/react
- Nix flakes

### Architectural Patterns
- Structure hexagonale preparee (`domain/`, `application/`, `adapters/`)
- Separation `adapters/driving/` (entrees) et `adapters/driven/` (sorties)

## Success Criteria
- [x] `npm run tauri dev` ouvre une fenetre Tauri avec un placeholder React
- [x] `cargo test --workspace` passe (0 tests, 0 erreurs)
- [x] `npx vitest run` passe
- [x] `cargo clippy --workspace -- -D warnings` zero warning
- [x] `npx oxlint .` zero erreur, zero warning
- [x] `nix develop` fournit Rust, Node, wasm32-wasip1 target
- [x] Structure hexagonale en place (`domain/`, `application/`, `adapters/driving/`, `adapters/driven/`)
- [x] shadcn/ui initialise avec au moins Button et Badge
- [x] Design tokens CSS (--accent, --sidebar-bg, fonts) definis

## Testing & Validation

### Manual Testing Steps
1. `nix develop` → verifier `rustc --version`, `node --version`, `cargo --version`
2. `npm install && npm run tauri dev` → fenetre Tauri s'ouvre
3. `cargo clippy --workspace -- -D warnings` → 0 warnings
4. `npx oxlint . && npx oxfmt --check .` → 0 erreurs

### Edge Cases
- WebKitGTK doit etre installe sur Linux (le flake.nix doit le fournir)
- Le target `wasm32-wasip1` doit etre disponible pour les futures taches plugin

## Dependencies

**Must complete first**: Aucune (premiere tache)

**Blocks**: Toutes les autres taches (02-28)

## Related Documentation
- **PRD**: §1 Vision produit (stack Tauri 2 + Rust + React)
- **ARCHI**: Tech Stack Summary, Folder Structure

---
**Estimated Time**: 2-3 hours
**Phase**: Foundation
