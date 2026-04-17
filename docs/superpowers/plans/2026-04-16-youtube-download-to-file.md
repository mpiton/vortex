# YouTube `download_to_file` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Permettre le téléchargement 1080p+ depuis YouTube en déléguant le download+merge DASH à yt-dlp via une nouvelle fonction `download_to_file` dans le plugin, puis enregistrer le fichier résultant comme téléchargement complété dans Vortex.

**Architecture:** Le plugin YouTube expose une nouvelle fonction WASM `download_to_file` qui lance yt-dlp avec `bestvideo+bestaudio --merge-output-format`, retourne le chemin du fichier fusionné. Côté Vortex, quand `resolve_stream_url` échoue avec `AdaptiveStreamOnly`, le moteur bascule sur `download_to_file`, déplace le fichier temp vers le dossier de téléchargements, et enregistre le download comme complété via un nouveau `RegisterLocalFileCommand`.

**Tech Stack:** Rust, extism-pdk 1.4, yt-dlp subprocess, Tauri 2 IPC, CQRS CommandBus, thiserror, serde_json

---

## Fichiers touchés

### Plugin (`vortex-mod-youtube/`)
| Fichier | Action | Responsabilité |
|---------|--------|----------------|
| `src/extractor.rs` | Modifier | Ajouter `yt_dlp_args_for_download_to_file` + `DEFAULT_DOWNLOAD_TIMEOUT_MS` + `parse_download_path_from_stdout` |
| `src/plugin_api.rs` | Modifier | Ajouter `#[plugin_fn] download_to_file` |
| `plugin.toml` | Modifier | Bump version `1.2.0` |
| `Cargo.toml` | Modifier | Bump version `1.2.0` |
| `CHANGELOG.md` | Modifier | Section `[1.2.0]` |

### Vortex core (`vortex/src-tauri/src/`)
| Fichier | Action | Responsabilité |
|---------|--------|----------------|
| `domain/error.rs` | Modifier | Ajouter variant `AdaptiveStreamOnly` |
| `domain/ports/driven/plugin_loader.rs` | Modifier | Ajouter `DownloadedFileInfo` + méthode `download_to_file` |
| `adapters/driven/plugin/extism_loader.rs` | Modifier | Implémenter `download_to_file` + détecter `AdaptiveStreamOnly` dans `resolve_stream_url` |
| `application/commands/mod.rs` | Modifier | Ajouter `RegisterLocalFileCommand` |
| `application/commands/register_local_file.rs` | Créer | Handler `handle_register_local_file` |
| `adapters/driving/tauri_ipc.rs` | Modifier | Fallback `AdaptiveStreamOnly` → `download_to_file` dans `download_media_start` |
| `vortex/CHANGELOG.md` | Modifier | Section `[Unreleased]` |

---

## Task 1 — Plugin : helpers `extractor.rs`

**Files:**
- Modify: `vortex-mod-youtube/src/extractor.rs`

- [ ] **Step 1 : Écrire les tests qui vont échouer**

Ajouter à la fin du bloc `#[cfg(test)] mod tests` dans `extractor.rs` :

```rust
#[test]
fn download_args_include_bestvideo_plus_bestaudio() {
    let args = yt_dlp_args_for_download_to_file("https://youtu.be/abc", "1080p", "mp4", "/tmp/vx", false);
    let fmt_idx = args.iter().position(|a| a == "--format").unwrap();
    assert!(args[fmt_idx + 1].contains("bestvideo"), "selector must start with bestvideo");
    assert!(args[fmt_idx + 1].contains("bestaudio"), "selector must include bestaudio");
}

#[test]
fn download_args_audio_only_uses_bestaudio() {
    let args = yt_dlp_args_for_download_to_file("https://youtu.be/abc", "", "m4a", "/tmp/vx", true);
    let fmt_idx = args.iter().position(|a| a == "--format").unwrap();
    assert!(args[fmt_idx + 1].starts_with("bestaudio"), "audio_only must start with bestaudio");
}

#[test]
fn download_args_include_merge_output_format() {
    let args = yt_dlp_args_for_download_to_file("https://youtu.be/abc", "1080p", "mp4", "/tmp/vx", false);
    assert!(args.contains(&"--merge-output-format".into()));
    let idx = args.iter().position(|a| a == "--merge-output-format").unwrap();
    assert_eq!(args[idx + 1], "mp4");
}

#[test]
fn download_args_include_output_template_with_dir() {
    let args = yt_dlp_args_for_download_to_file("https://youtu.be/abc", "720p", "mp4", "/tmp/vx", false);
    let out_idx = args.iter().position(|a| a == "--output").unwrap();
    assert!(args[out_idx + 1].starts_with("/tmp/vx/"), "output template must be in output_dir");
}

#[test]
fn download_args_include_print_after_move() {
    let args = yt_dlp_args_for_download_to_file("https://youtu.be/abc", "1080p", "mp4", "/tmp/vx", false);
    let idx = args.iter().position(|a| a == "--print").unwrap();
    assert_eq!(args[idx + 1], "after_move:%(filepath)s");
}

#[test]
fn parse_download_path_returns_last_nonempty_line() {
    let stdout = "\n/tmp/vx/dQw4w9WgXcQ.mp4\n";
    let path = parse_download_path_from_stdout(stdout).unwrap();
    assert_eq!(path, "/tmp/vx/dQw4w9WgXcQ.mp4");
}

#[test]
fn parse_download_path_empty_stdout_returns_error() {
    let result = parse_download_path_from_stdout("   \n  \n");
    assert!(matches!(result, Err(PluginError::NoMatchingFormat)));
}
```

- [ ] **Step 2 : Vérifier que les tests échouent**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo test 2>&1 | grep -E "FAILED|error\[" | head -20
```
Attendu : erreurs de compilation (fonctions manquantes).

- [ ] **Step 3 : Implémenter les fonctions dans `extractor.rs`**

Ajouter après la constante `DEFAULT_TIMEOUT_MS` existante (ligne 28) :

```rust
/// Default timeout for full video download+merge — 30 minutes.
pub const DEFAULT_DOWNLOAD_TIMEOUT_MS: u64 = 1_800_000;
```

Ajouter après `yt_dlp_args_for_stream_url` (avant la section `// ── Tests`) :

```rust
/// Build yt-dlp args for a full download+merge operation.
///
/// Unlike `yt_dlp_args_for_stream_url`, this actually downloads the video
/// and audio streams, merges them via ffmpeg (spawned internally by yt-dlp),
/// and writes the final file to `output_dir`. The merged file path is printed
/// to stdout via `--print after_move:%(filepath)s` for the caller to read.
///
/// The format selector prefers DASH streams (bestvideo+bestaudio) which allow
/// 1080p and above, unlike the `best[protocol=https]` selector used by
/// `resolve_stream_url` which is limited to pre-merged ≤720p streams.
pub fn yt_dlp_args_for_download_to_file(
    url: &str,
    quality: &str,
    format: &str,
    output_dir: &str,
    audio_only: bool,
) -> Vec<String> {
    let selector = build_download_format_selector(quality, format, audio_only);
    let merge_format = if audio_only { format } else { format };
    let output_template = format!("{output_dir}/%(id)s.%(ext)s");

    vec![
        "--format".into(),
        selector,
        "--merge-output-format".into(),
        merge_format.into(),
        "--output".into(),
        output_template,
        "--print".into(),
        "after_move:%(filepath)s".into(),
        "--no-playlist".into(),
        "--no-warnings".into(),
        "--".into(),
        url.into(),
    ]
}

/// Build a yt-dlp format selector for DASH download+merge.
///
/// For video: prefers `bestvideo[height<=H]+bestaudio`, which selects
/// the best DASH video/audio streams up to the requested height and lets
/// yt-dlp merge them via ffmpeg. Falls back to `best[height<=H]` for
/// services that only offer pre-merged streams.
///
/// For audio-only: uses `bestaudio`.
fn build_download_format_selector(quality: &str, format: &str, audio_only: bool) -> String {
    let height: Option<u32> = quality.trim_end_matches('p').parse().ok();
    let has_format = !format.is_empty() && format.chars().all(|c| c.is_ascii_alphanumeric());

    if audio_only {
        if has_format {
            format!("bestaudio[ext={format}]/bestaudio")
        } else {
            "bestaudio".into()
        }
    } else {
        match height {
            Some(h) => format!(
                "bestvideo[height<={h}]+bestaudio/bestvideo[height<={h}]+bestaudio[ext=m4a]/best[height<={h}]"
            ),
            None => "bestvideo+bestaudio/best".into(),
        }
    }
}

/// Parse the final merged file path from yt-dlp stdout.
///
/// With `--print after_move:%(filepath)s`, yt-dlp appends one line to stdout
/// containing the absolute path of the merged output file. We take the last
/// non-empty line to be robust against any incidental output.
pub fn parse_download_path_from_stdout(stdout: &str) -> Result<String, PluginError> {
    stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
        .ok_or(PluginError::NoMatchingFormat)
}
```

- [ ] **Step 4 : Vérifier que les tests passent**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo test extractor 2>&1 | tail -20
```
Attendu : tous les tests `extractor::tests::download_args_*` et `parse_download_path_*` PASS.

- [ ] **Step 5 : Lint**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo clippy -- -D warnings 2>&1 | tail -20
```
Attendu : aucun warning.

- [ ] **Step 6 : Commit**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
git add src/extractor.rs
git commit -m "feat(extractor): add yt_dlp_args_for_download_to_file and parse helpers"
```

---

## Task 2 — Plugin : fonction WASM `download_to_file`

**Files:**
- Modify: `vortex-mod-youtube/src/plugin_api.rs`

- [ ] **Step 1 : Implémenter `download_to_file` dans `plugin_api.rs`**

Ajouter l'import en tête du fichier (après les imports existants) :

```rust
use crate::extractor::{
    build_subprocess_request, parse_subprocess_response,
    yt_dlp_args_for_download_to_file, parse_download_path_from_stdout,
    DEFAULT_DOWNLOAD_TIMEOUT_MS,
};
```

> Note : `DEFAULT_DOWNLOAD_TIMEOUT_MS` n'est pas encore dans les imports existants — ajouter uniquement si absent.

Ajouter la fonction après `resolve_stream_url` (avant `call_yt_dlp`) :

```rust
/// Download a video/audio file using yt-dlp's native download+merge pipeline.
///
/// Use this when `resolve_stream_url` returns `AdaptiveStreamOnly` — i.e. when
/// the requested quality is only available as DASH streams that must be merged
/// with ffmpeg. yt-dlp handles the multi-stream download and ffmpeg merge
/// internally; the merged file is written to `output_dir` and its path is
/// returned.
///
/// Input: JSON `{ "url", "quality"?, "format"?, "output_dir", "audio_only"? }`
/// Output: absolute path of the merged file (raw string, not JSON)
#[plugin_fn]
pub fn download_to_file(input: String) -> FnResult<String> {
    #[derive(serde::Deserialize)]
    struct Input {
        url: String,
        #[serde(default)]
        quality: String,
        #[serde(default)]
        format: String,
        output_dir: String,
        #[serde(default)]
        audio_only: bool,
    }

    let params: Input =
        serde_json::from_str(&input).map_err(|e| error_to_fn_error(PluginError::SerdeJson(e)))?;

    ensure_single_video(&params.url).map_err(error_to_fn_error)?;

    let args = yt_dlp_args_for_download_to_file(
        &params.url,
        &params.quality,
        &params.format,
        &params.output_dir,
        params.audio_only,
    );

    // Override timeout: full download+merge can take 30+ minutes.
    let req = crate::extractor::SubprocessRequest {
        binary: "yt-dlp".into(),
        args,
        timeout_ms: DEFAULT_DOWNLOAD_TIMEOUT_MS,
    };
    let req_json = serde_json::to_string(&req)
        .map_err(|e| error_to_fn_error(PluginError::SerdeJson(e)))?;

    let resp_json = unsafe { run_subprocess(req_json)? };
    let stdout = parse_subprocess_response(&resp_json).map_err(error_to_fn_error)?;

    parse_download_path_from_stdout(&stdout).map_err(error_to_fn_error)
}
```

> **Note** : `SubprocessRequest` est actuellement `pub(crate)` dans `extractor.rs`. Changer sa visibilité en `pub` pour que `plugin_api.rs` puisse l'utiliser directement. Alternativement, exposer un helper `build_subprocess_request_with_timeout(args, timeout_ms)`.

- [ ] **Step 2 : Ajuster la visibilité de `SubprocessRequest` dans `extractor.rs`**

Dans `vortex-mod-youtube/src/extractor.rs`, ligne 12, changer :
```rust
pub struct SubprocessRequest {
```
(vérifier qu'il est déjà `pub` — si oui, rien à faire).

Si `SubprocessRequest` est `pub(crate)`, le rendre `pub`.

- [ ] **Step 3 : Compiler**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo build 2>&1 | tail -20
```
Attendu : compilation réussie.

- [ ] **Step 4 : Lint**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo clippy -- -D warnings 2>&1 | tail -20
```
Attendu : aucun warning.

- [ ] **Step 5 : Commit**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
git add src/plugin_api.rs src/extractor.rs
git commit -m "feat(plugin): add download_to_file WASM export for DASH+merge support"
```

---

## Task 3 — Plugin : version bump + CHANGELOG

**Files:**
- Modify: `vortex-mod-youtube/plugin.toml`
- Modify: `vortex-mod-youtube/Cargo.toml`
- Modify: `vortex-mod-youtube/CHANGELOG.md` (créer si absent)

- [ ] **Step 1 : Bumper `plugin.toml` vers `1.2.0`**

Dans `plugin.toml`, ligne 3 :
```toml
version = "1.2.0"
```

- [ ] **Step 2 : Bumper `Cargo.toml` vers `1.2.0`**

Dans `Cargo.toml`, ligne 3 :
```toml
version = "1.2.0"
```

- [ ] **Step 3 : Mettre à jour / créer `CHANGELOG.md`**

Si `CHANGELOG.md` n'existe pas, créer avec ce contenu.  
Si il existe, ajouter la section `[1.2.0]` en haut (après le titre) :

```markdown
## [1.2.0] - 2026-04-16

### Added
- `download_to_file` plugin function: delegates DASH download + ffmpeg merge to
  yt-dlp, enabling true 1080p/1440p/2160p downloads. Called by Vortex core when
  `resolve_stream_url` returns `AdaptiveStreamOnly` (i.e. when YouTube only
  offers the requested quality as separate video+audio DASH streams).

### Changed
- `DEFAULT_DOWNLOAD_TIMEOUT_MS` set to 30 minutes for `download_to_file`
  (vs 60 seconds for `resolve_stream_url`).
```

- [ ] **Step 4 : Vérifier le build complet**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo test 2>&1 | tail -10
```
Attendu : tous les tests PASS.

- [ ] **Step 5 : Commit**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
git add plugin.toml Cargo.toml CHANGELOG.md
git commit -m "chore(release): bump to 1.2.0 — add download_to_file"
```

---

## Task 4 — Vortex core : `DomainError::AdaptiveStreamOnly`

**Files:**
- Modify: `vortex/src-tauri/src/domain/error.rs`

- [ ] **Step 1 : Écrire le test qui va échouer**

Dans `domain/error.rs`, ajouter dans `#[cfg(test)] mod tests` :

```rust
#[test]
fn test_display_adaptive_stream_only() {
    let err = DomainError::AdaptiveStreamOnly;
    assert_eq!(
        err.to_string(),
        "Video is only available as adaptive stream (DASH/HLS); use download_to_file"
    );
}
```

- [ ] **Step 2 : Vérifier que le test échoue**

```bash
cd /home/matvei/projets/vx/vortex
cargo test domain::error -- --nocapture 2>&1 | tail -10
```
Attendu : erreur de compilation (variant manquant).

- [ ] **Step 3 : Ajouter le variant**

Dans `domain/error.rs`, ajouter dans l'enum `DomainError` (après `PluginError`) :

```rust
AdaptiveStreamOnly,
```

Dans `impl std::fmt::Display for DomainError`, ajouter dans le `match` :

```rust
DomainError::AdaptiveStreamOnly => write!(
    f,
    "Video is only available as adaptive stream (DASH/HLS); use download_to_file"
),
```

- [ ] **Step 4 : Vérifier que le test passe**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_display_adaptive_stream_only 2>&1 | tail -10
```
Attendu : PASS.

- [ ] **Step 5 : Vérifier que l'existant compiletoujours**

```bash
cd /home/matvei/projets/vx/vortex
cargo build --workspace 2>&1 | grep -E "^error" | head -10
```
Attendu : aucune erreur (le compilateur signalera les `match` non exhaustifs si des places utilisent `DomainError` avec match exhaustif — corriger chaque cas).

- [ ] **Step 6 : Corriger les matchs non exhaustifs**

Si le compilateur signale des matchs non exhaustifs (ex : dans des `impl From<DomainError>`), ajouter le bras :
```rust
DomainError::AdaptiveStreamOnly => { /* même traitement que PluginError */ }
```

- [ ] **Step 7 : Commit**

```bash
cd /home/matvei/projets/vx/vortex
git add src-tauri/src/domain/error.rs
git commit -m "feat(domain): add AdaptiveStreamOnly error variant"
```

---

## Task 5 — Vortex core : trait `PluginLoader` + `DownloadedFileInfo`

**Files:**
- Modify: `vortex/src-tauri/src/domain/ports/driven/plugin_loader.rs`

- [ ] **Step 1 : Écrire le test qui va échouer**

Ajouter dans `plugin_loader.rs` (ou un fichier de test dédié dans `domain/ports/driven/tests.rs` si un tel fichier existe) :

```rust
#[cfg(test)]
mod plugin_loader_tests {
    use super::*;
    use crate::domain::error::DomainError;

    struct MinimalLoader;
    impl PluginLoader for MinimalLoader {
        fn load(&self, _: &crate::domain::model::plugin::PluginManifest) -> Result<(), DomainError> { Ok(()) }
        fn unload(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
        fn resolve_url(&self, _: &str) -> Result<Option<crate::domain::model::plugin::PluginInfo>, DomainError> { Ok(None) }
        fn list_loaded(&self) -> Result<Vec<crate::domain::model::plugin::PluginInfo>, DomainError> { Ok(vec![]) }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> { Ok(()) }
    }

    #[test]
    fn test_download_to_file_default_returns_not_found() {
        let loader = MinimalLoader;
        let result = loader.download_to_file("https://youtu.be/x", "1080p", "mp4", "/tmp", false);
        assert!(matches!(result, Err(DomainError::NotFound(_))));
    }
}
```

- [ ] **Step 2 : Vérifier que le test échoue**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_download_to_file_default_returns_not_found 2>&1 | tail -10
```
Attendu : erreur de compilation (méthode manquante).

- [ ] **Step 3 : Ajouter `DownloadedFileInfo` et la méthode `download_to_file`**

Dans `plugin_loader.rs`, ajouter avant le trait `PluginLoader` :

```rust
/// Result of a `download_to_file` plugin call.
pub struct DownloadedFileInfo {
    /// Absolute path to the merged output file on the host filesystem.
    pub path: std::path::PathBuf,
    /// File size in bytes (obtained from host `std::fs::metadata`).
    pub size: u64,
}
```

Ajouter dans le trait `PluginLoader` (après `resolve_stream_url`) :

```rust
/// Download a video/audio file using the plugin's native download+merge
/// pipeline (e.g. yt-dlp DASH). Used as fallback when `resolve_stream_url`
/// returns `AdaptiveStreamOnly`.
///
/// The plugin downloads both streams to `output_dir`, merges them, and
/// returns the absolute path of the merged file. The host then reads the
/// file size via `std::fs::metadata`.
///
/// Returns `Err(DomainError::NotFound)` by default (adapters that do not
/// support this operation should rely on the default).
fn download_to_file(
    &self,
    _url: &str,
    _quality: &str,
    _format: &str,
    _output_dir: &str,
    _audio_only: bool,
) -> Result<DownloadedFileInfo, DomainError> {
    Err(DomainError::NotFound(
        "download_to_file not supported by this loader".into(),
    ))
}
```

- [ ] **Step 4 : Vérifier que le test passe**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_download_to_file_default_returns_not_found 2>&1 | tail -10
```
Attendu : PASS.

- [ ] **Step 5 : Commit**

```bash
cd /home/matvei/projets/vx/vortex
git add src-tauri/src/domain/ports/driven/plugin_loader.rs
git commit -m "feat(domain): add DownloadedFileInfo and download_to_file to PluginLoader trait"
```

---

## Task 6 — Vortex core : `ExtismPluginLoader` — implémenter `download_to_file` + détecter `AdaptiveStreamOnly`

**Files:**
- Modify: `vortex/src-tauri/src/adapters/driven/plugin/extism_loader.rs`

- [ ] **Step 1 : Écrire les tests qui vont échouer**

Dans le module `#[cfg(test)] mod tests` de `extism_loader.rs`, ajouter :

```rust
#[test]
fn test_resolve_stream_url_maps_adaptive_stream_error() {
    // Verify that a PluginError containing "adaptive stream" in its message
    // gets mapped to DomainError::AdaptiveStreamOnly.
    let msg = "video is only available as an adaptive stream (HLS/DASH) at this quality; try 360p or 480p for a direct download";
    assert!(is_adaptive_stream_error(msg));
}

#[test]
fn test_resolve_stream_url_does_not_map_other_errors() {
    assert!(!is_adaptive_stream_error("no format matches requested quality"));
    assert!(!is_adaptive_stream_error("yt-dlp failed (exit code 1): video unavailable"));
}
```

- [ ] **Step 2 : Vérifier que les tests échouent**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_resolve_stream_url_maps_adaptive 2>&1 | tail -10
```
Attendu : erreur de compilation.

- [ ] **Step 3 : Ajouter le helper `is_adaptive_stream_error` et implémenter `download_to_file`**

Dans `extism_loader.rs`, ajouter après `impl PluginLoader for ExtismPluginLoader` (dans le bloc impl) :

```rust
fn resolve_stream_url(
    &self,
    url: &str,
    quality: &str,
    format: &str,
    audio_only: bool,
) -> Result<String, DomainError> {
    // Find the plugin that claims this URL.
    let info = self
        .resolve_url(url)?
        .ok_or_else(|| DomainError::PluginError(format!("no plugin can handle URL: {url}")))?;

    if info.name() == "builtin-http" {
        return Err(DomainError::NotFound("builtin-http".into()));
    }

    let input = serde_json::json!({
        "url": url,
        "quality": quality,
        "format": format,
        "audio_only": audio_only,
    })
    .to_string();

    self.registry
        .call_plugin(info.name(), "resolve_stream_url", &input)
        .map_err(|e| {
            let msg = e.to_string();
            if is_adaptive_stream_error(&msg) {
                DomainError::AdaptiveStreamOnly
            } else {
                DomainError::PluginError(format!(
                    "plugin '{}' resolve_stream_url failed: {msg}",
                    info.name()
                ))
            }
        })
}

fn download_to_file(
    &self,
    url: &str,
    quality: &str,
    format: &str,
    output_dir: &str,
    audio_only: bool,
) -> Result<crate::domain::ports::driven::DownloadedFileInfo, DomainError> {
    let info = self
        .resolve_url(url)?
        .ok_or_else(|| DomainError::PluginError(format!("no plugin can handle URL: {url}")))?;

    if info.name() == "builtin-http" {
        return Err(DomainError::NotFound("builtin-http".into()));
    }

    let input = serde_json::json!({
        "url": url,
        "quality": quality,
        "format": format,
        "output_dir": output_dir,
        "audio_only": audio_only,
    })
    .to_string();

    let path_str = self
        .registry
        .call_plugin(info.name(), "download_to_file", &input)
        .map_err(|e| {
            DomainError::PluginError(format!(
                "plugin '{}' download_to_file failed: {e}",
                info.name()
            ))
        })?;

    let path = std::path::PathBuf::from(path_str.trim());

    // Validate the returned path is within output_dir (path traversal protection).
    let canon_output = std::path::Path::new(output_dir)
        .canonicalize()
        .map_err(|e| DomainError::StorageError(format!("output_dir invalid: {e}")))?;
    let canon_path = path
        .canonicalize()
        .map_err(|e| DomainError::StorageError(format!("returned path invalid: {e}")))?;
    if !canon_path.starts_with(&canon_output) {
        return Err(DomainError::ValidationError(format!(
            "plugin returned path outside output_dir: {}",
            path.display()
        )));
    }

    let size = std::fs::metadata(&canon_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(crate::domain::ports::driven::DownloadedFileInfo {
        path: canon_path,
        size,
    })
}
```

Ajouter la fonction libre (hors du bloc `impl`) :

```rust
/// Returns `true` if the plugin error message indicates an adaptive-only stream.
///
/// Matches the exact error text emitted by `vortex-mod-youtube`'s
/// `PluginError::AdaptiveStreamOnly` variant. Kept as a named function so
/// it can be unit-tested independently of the Extism runtime.
fn is_adaptive_stream_error(msg: &str) -> bool {
    msg.contains("adaptive stream")
}
```

> **Note :** Remplacer l'implémentation existante de `resolve_stream_url` dans le `impl PluginLoader for ExtismPluginLoader` — ne pas dupliquer.

- [ ] **Step 4 : Vérifier que les tests passent**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_resolve_stream_url_maps_adaptive test_resolve_stream_url_does_not_map 2>&1 | tail -10
```
Attendu : PASS.

- [ ] **Step 5 : Tous les tests du workspace**

```bash
cd /home/matvei/projets/vx/vortex
cargo test --workspace 2>&1 | tail -20
```
Attendu : aucun FAILED.

- [ ] **Step 6 : Commit**

```bash
cd /home/matvei/projets/vx/vortex
git add src-tauri/src/adapters/driven/plugin/extism_loader.rs
git commit -m "feat(plugin): implement download_to_file in ExtismPluginLoader + detect AdaptiveStreamOnly"
```

---

## Task 7 — Vortex core : `RegisterLocalFileCommand` + handler

**Files:**
- Modify: `vortex/src-tauri/src/application/commands/mod.rs`
- Create: `vortex/src-tauri/src/application/commands/register_local_file.rs`

- [ ] **Step 1 : Écrire les tests qui vont échouer**

Créer `vortex/src-tauri/src/application/commands/register_local_file.rs` avec les tests en premier :

```rust
//! Handler for `RegisterLocalFileCommand`.
//!
//! Registers an already-downloaded local file as a Completed download.
//! Used after `download_to_file` produces a merged file via yt-dlp.

use std::path::PathBuf;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, Url};

impl CommandBus {
    pub async fn handle_register_local_file(
        &self,
        cmd: super::RegisterLocalFileCommand,
    ) -> Result<DownloadId, AppError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::RegisterLocalFileCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository,
        EventBus, FileStorage, HttpClient, PluginLoader,
    };
    use std::sync::Arc;

    // ── Minimal mocks (copies from start_download.rs tests) ──────────────────

    struct MockRepo(Mutex<HashMap<u64, Download>>);
    impl MockRepo {
        fn new() -> Self { Self(Mutex::new(HashMap::new())) }
    }
    impl DownloadRepository for MockRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.0.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.0.lock().unwrap().insert(d.id().0, d.clone()); Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.0.lock().unwrap().remove(&id.0); Ok(())
        }
        fn find_by_state(&self, s: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self.0.lock().unwrap().values().filter(|d| d.state() == s).cloned().collect())
        }
    }
    struct MockEngine;
    impl DownloadEngine for MockEngine {
        fn start(&self, _: &Download) -> Result<(), DomainError> { Ok(()) }
        fn pause(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
        fn resume(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
        fn cancel(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockBus(Mutex<Vec<DomainEvent>>);
    impl MockBus { fn new() -> Self { Self(Mutex::new(vec![])) } }
    impl EventBus for MockBus {
        fn publish(&self, e: DomainEvent) { self.0.lock().unwrap().push(e); }
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }
    struct MockHttp;
    impl HttpClient for MockHttp {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> { Err(DomainError::NetworkError("no".into())) }
        fn get_range(&self, _: &str, s: u64, e: u64) -> Result<Vec<u8>, DomainError> { Ok(vec![0u8; (e - s + 1) as usize]) }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> { Ok(false) }
    }
    struct MockFs;
    impl FileStorage for MockFs {
        fn create_file(&self, _: &std::path::Path, _: u64) -> Result<(), DomainError> { Ok(()) }
        fn write_segment(&self, _: &std::path::Path, _: u64, _: &[u8]) -> Result<(), DomainError> { Ok(()) }
        fn read_meta(&self, _: &std::path::Path) -> Result<Option<DownloadMeta>, DomainError> { Ok(None) }
        fn write_meta(&self, _: &std::path::Path, _: &DownloadMeta) -> Result<(), DomainError> { Ok(()) }
        fn delete_meta(&self, _: &std::path::Path) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockPlugin;
    impl PluginLoader for MockPlugin {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> { Ok(()) }
        fn unload(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> { Ok(None) }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> { Ok(vec![]) }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockCfg;
    impl ConfigStore for MockCfg {
        fn get_config(&self) -> Result<AppConfig, DomainError> { Ok(AppConfig::default()) }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> { Ok(AppConfig::default()) }
    }
    struct MockCred;
    impl CredentialStore for MockCred {
        fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> { Ok(None) }
        fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> { Ok(()) }
        fn delete(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockClip;
    impl ClipboardObserver for MockClip {
        fn start(&self) -> Result<(), DomainError> { Ok(()) }
        fn stop(&self) -> Result<(), DomainError> { Ok(()) }
        fn get_urls(&self) -> Result<Vec<String>, DomainError> { Ok(vec![]) }
    }
    struct FakeArchive;
    impl crate::domain::ports::driven::ArchiveExtractor for FakeArchive {
        fn detect_format(&self, _: &std::path::Path) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> { Ok(None) }
        fn can_extract(&self, _: &std::path::Path) -> Result<bool, DomainError> { Ok(false) }
        fn extract(&self, _: &std::path::Path, _: &std::path::Path, _: Option<&str>) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary { extracted_files: 0, extracted_bytes: 0, duration_ms: 0, warnings: vec![] })
        }
        fn list_contents(&self, _: &std::path::Path, _: Option<&str>) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> { Ok(vec![]) }
        fn detect_segments(&self, _: &std::path::Path) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> { Ok(None) }
    }

    fn make_bus() -> (CommandBus, Arc<MockRepo>, Arc<MockBus>) {
        let repo = Arc::new(MockRepo::new());
        let events = Arc::new(MockBus::new());
        let bus = CommandBus::new(
            repo.clone(), Arc::new(MockEngine), events.clone(),
            Arc::new(MockFs), Arc::new(MockHttp), Arc::new(MockPlugin),
            Arc::new(MockCfg), Arc::new(MockCred), Arc::new(MockClip),
            Arc::new(FakeArchive), None,
        );
        (bus, repo, events)
    }

    #[tokio::test]
    async fn test_register_local_file_creates_completed_download() {
        let (bus, repo, _) = make_bus();

        let cmd = RegisterLocalFileCommand {
            source_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            destination_path: PathBuf::from("/tmp/downloads/video.mp4"),
            filename: "Rick Astley - Never Gonna Give You Up.mp4".to_string(),
            source_hostname: Some("www.youtube.com".to_string()),
            file_size: 52_428_800,
        };

        let id = bus.handle_register_local_file(cmd).await.unwrap();

        let saved = repo.0.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(saved.state(), DownloadState::Completed);
        assert_eq!(saved.file_name(), "Rick Astley - Never Gonna Give You Up.mp4");
        assert_eq!(saved.source_hostname(), "www.youtube.com");
    }

    #[tokio::test]
    async fn test_register_local_file_emits_created_and_completed_events() {
        let (bus, _, events) = make_bus();

        let cmd = RegisterLocalFileCommand {
            source_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            destination_path: PathBuf::from("/tmp/downloads/video.mp4"),
            filename: "video.mp4".to_string(),
            source_hostname: None,
            file_size: 0,
        };

        let id = bus.handle_register_local_file(cmd).await.unwrap();

        let evs = events.0.lock().unwrap();
        assert!(evs.iter().any(|e| *e == DomainEvent::DownloadCreated { id }), "must emit DownloadCreated");
        assert!(evs.iter().any(|e| *e == DomainEvent::DownloadCompleted { id }), "must emit DownloadCompleted");
    }
}
```

- [ ] **Step 2 : Vérifier que les tests échouent**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_register_local_file 2>&1 | tail -20
```
Attendu : erreur de compilation (`RegisterLocalFileCommand` manquant, `todo!()`).

- [ ] **Step 3 : Ajouter `RegisterLocalFileCommand` dans `commands/mod.rs`**

Dans `commands/mod.rs`, ajouter :
- `mod register_local_file;` dans la liste des modules (après `mod start_download;`)
- La struct de commande (à la fin du fichier) :

```rust
#[derive(Debug)]
pub struct RegisterLocalFileCommand {
    /// Original source URL (e.g. "https://www.youtube.com/watch?v=...")
    /// used to populate the download record's URL field.
    pub source_url: String,
    /// Absolute path where the merged file has been moved by the caller.
    pub destination_path: PathBuf,
    /// Final filename (e.g. "Rick Astley - Never Gonna Give You Up.mp4").
    pub filename: String,
    /// Origin hostname override (e.g. "www.youtube.com").
    pub source_hostname: Option<String>,
    /// File size in bytes.
    pub file_size: u64,
}
impl Command for RegisterLocalFileCommand {}
```

- [ ] **Step 4 : Implémenter `handle_register_local_file` dans `register_local_file.rs`**

Remplacer le `todo!()` par l'implémentation :

```rust
impl CommandBus {
    pub async fn handle_register_local_file(
        &self,
        cmd: super::RegisterLocalFileCommand,
    ) -> Result<DownloadId, AppError> {
        let url = Url::new(&cmd.source_url)?;

        let id = next_download_id();
        let dest = cmd.destination_path.to_string_lossy().to_string();

        let mut download = Download::new(id, url, cmd.filename, dest);

        if let Some(hostname) = cmd.source_hostname {
            download = download.with_source_hostname(hostname);
        }
        if cmd.file_size > 0 {
            download.set_file_size(cmd.file_size);
        }

        // Transition: Queued → Downloading → Completed
        download.start().map_err(|e| AppError::Domain(e))?;
        let completed_event = download.complete().map_err(|e| AppError::Domain(e))?;

        self.download_repo().save(&download)?;
        self.event_bus().publish(DomainEvent::DownloadCreated { id });
        self.event_bus().publish(completed_event);

        Ok(id)
    }
}
```

Ajouter la fonction `next_download_id` — elle est déjà définie dans `start_download.rs` mais est privée. Options :
- La déplacer dans un module partagé `commands/id_gen.rs`, ou
- La dupliquer dans `register_local_file.rs` (YAGNI — deux usages ne justifient pas encore une abstraction si les fichiers sont proches).

**Choisir la duplication** (deux fichiers, même module) :

```rust
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_LOCAL_SEQ: AtomicU64 = AtomicU64::new(0);

fn next_download_id() -> DownloadId {
    let seq = NEXT_LOCAL_SEQ.fetch_add(1, Ordering::Relaxed) & 0xFFF;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    DownloadId((ts << 12) | seq)
}
```

> **Note** : Si `Download::set_file_size` n'existe pas encore, ajouter dans `domain/model/download.rs` :
> ```rust
> pub fn set_file_size(&mut self, bytes: u64) {
>     self.file_size = Some(crate::domain::model::download::FileSize(bytes));
>     self.downloaded_bytes = bytes;
> }
> ```

- [ ] **Step 5 : Vérifier que les tests passent**

```bash
cd /home/matvei/projets/vx/vortex
cargo test test_register_local_file 2>&1 | tail -10
```
Attendu : 2 tests PASS.

- [ ] **Step 6 : Tous les tests workspace**

```bash
cd /home/matvei/projets/vx/vortex
cargo test --workspace 2>&1 | grep -E "FAILED|test result" | tail -10
```
Attendu : 0 FAILED.

- [ ] **Step 7 : Commit**

```bash
cd /home/matvei/projets/vx/vortex
git add src-tauri/src/application/commands/mod.rs src-tauri/src/application/commands/register_local_file.rs src-tauri/src/domain/model/download.rs
git commit -m "feat(commands): add RegisterLocalFileCommand for yt-dlp merged downloads"
```

---

## Task 8 — Vortex core : `tauri_ipc.rs` — fallback `AdaptiveStreamOnly`

**Files:**
- Modify: `vortex/src-tauri/src/adapters/driving/tauri_ipc.rs`

- [ ] **Step 1 : Modifier `download_media_start`**

Remplacer la section `let stream_url = tokio::task::spawn_blocking(...)...await...?;` et la section `let cmd = StartDownloadCommand { ... }` + `state.command_bus.handle_start_download(cmd)...`

par :

```rust
// Plugin calls are synchronous (Extism runs inside a Mutex). Run on the
// blocking thread pool so we don't starve the async executor.
enum StreamResolution {
    CdnUrl(String),
    LocalFile {
        path: std::path::PathBuf,
        size: u64,
        filename: String,
    },
}

let plugin_loader = state.plugin_loader.clone();
let url_clone = url.clone();
let quality_clone = quality.clone();
let format_clone = format.clone();
let title_clone = title.clone();

let resolution = tokio::task::spawn_blocking(move || -> Result<StreamResolution, String> {
    match plugin_loader.resolve_stream_url(
        &url_clone,
        &quality_clone,
        &format_clone,
        audio_only,
    ) {
        Ok(cdn_url) => Ok(StreamResolution::CdnUrl(cdn_url)),

        Err(crate::domain::error::DomainError::AdaptiveStreamOnly) => {
            // yt-dlp must handle the full download+merge.
            let temp_dir = std::env::temp_dir().join("vortex-downloads");
            std::fs::create_dir_all(&temp_dir).map_err(|e| format!("failed to create temp dir: {e}"))?;

            let file_info = plugin_loader
                .download_to_file(
                    &url_clone,
                    &quality_clone,
                    &format_clone,
                    temp_dir.to_str().unwrap_or("/tmp/vortex-downloads"),
                    audio_only,
                )
                .map_err(|e| format!("download_to_file failed: {e}"))?;

            // Determine final filename: prefer title override, else keep yt-dlp's name.
            let filename = title_clone
                .as_deref()
                .filter(|t| !t.trim().is_empty())
                .map(|t| format!("{}.{}", sanitize_filename(t), format_clone))
                .unwrap_or_else(|| {
                    file_info
                        .path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("download")
                        .to_string()
                });

            // Determine final destination directory.
            let dest_dir = dirs::download_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            let dest_path = dest_dir.join(&filename);

            // Atomic move (same filesystem) → fallback copy+delete.
            if std::fs::rename(&file_info.path, &dest_path).is_err() {
                std::fs::copy(&file_info.path, &dest_path)
                    .map_err(|e| format!("failed to copy merged file: {e}"))?;
                let _ = std::fs::remove_file(&file_info.path);
            }

            Ok(StreamResolution::LocalFile {
                path: dest_path,
                size: file_info.size,
            })
        }

        Err(crate::domain::error::DomainError::NotFound(_)) => {
            if is_known_media_platform(&url_clone) {
                Err(
                    "No media plugin installed for this URL. \
                     Open the Plugin Store and install the appropriate plugin (e.g. vortex-mod-youtube)."
                        .to_string(),
                )
            } else {
                Ok(StreamResolution::CdnUrl(url_clone))
            }
        }

        Err(e) => Err(format!("Failed to resolve stream URL: {e}")),
    }
})
.await
.map_err(|e| format!("Task join error: {e}"))??;

match resolution {
    StreamResolution::CdnUrl(stream_url) => {
        let filename = title
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .map(|t| format!("{}.{}", sanitize_filename(t), format));

        let cmd = crate::application::commands::StartDownloadCommand {
            url: stream_url,
            destination: None,
            filename,
            source_hostname_override,
        };
        state
            .command_bus
            .handle_start_download(cmd)
            .await
            .map(|id| id.0)
            .map_err(|e| e.to_string())
    }

    StreamResolution::LocalFile { path, size, filename } => {
        let cmd = crate::application::commands::RegisterLocalFileCommand {
            source_url: url,
            destination_path: path,
            filename,
            source_hostname: source_hostname_override,
            file_size: size,
        };
        state
            .command_bus
            .handle_register_local_file(cmd)
            .await
            .map(|id| id.0)
            .map_err(|e| e.to_string())
    }
}
```

> **Note** : L'enum `StreamResolution` doit être déclarée **à l'intérieur** de la fonction `download_media_start` (pas au niveau du module) pour rester localisée.

- [ ] **Step 2 : Supprimer le code mort**

Supprimer les lignes devenues inutilisées (l'ancien bloc `let stream_url`, `let filename`, `let cmd = StartDownloadCommand...`, `state.command_bus.handle_start_download(...)`).

- [ ] **Step 3 : Compiler**

```bash
cd /home/matvei/projets/vx/vortex
cargo build --workspace 2>&1 | grep -E "^error" | head -20
```
Attendu : compilation sans erreur.

- [ ] **Step 4 : Tous les tests**

```bash
cd /home/matvei/projets/vx/vortex
cargo test --workspace 2>&1 | grep -E "FAILED|test result" | tail -10
```
Attendu : 0 FAILED.

- [ ] **Step 5 : Lint**

```bash
cd /home/matvei/projets/vx/vortex
cargo clippy --workspace -- -D warnings 2>&1 | grep "^error" | head -10
```
Attendu : 0 erreur.

- [ ] **Step 6 : Mettre à jour `vortex/CHANGELOG.md`**

Dans la section `[Unreleased]` :

```markdown
### Added
- YouTube 1080p+ support: when `resolve_stream_url` returns `AdaptiveStreamOnly`,
  `download_media_start` now falls back to `download_to_file` which delegates the
  full DASH download + ffmpeg merge to yt-dlp. The merged file is moved to the
  downloads folder and registered as a completed download.

### Fixed
- YouTube downloads silently downgrading to 360p when 1080p was requested but only
  DASH streams were available.
```

- [ ] **Step 7 : Commit**

```bash
cd /home/matvei/projets/vx/vortex
git add src-tauri/src/adapters/driving/tauri_ipc.rs CHANGELOG.md
git commit -m "feat(download): fallback to download_to_file when AdaptiveStreamOnly (fixes 1080p YouTube)"
```

---

## Task 9 — Build plugin, release, mise à jour registry

- [ ] **Step 1 : Build WASM**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
cargo build --target wasm32-wasip1 --release 2>&1 | tail -10
```
Attendu : `Finished release [optimized] target(s)`.

- [ ] **Step 2 : Calculer les checksums**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
sha256sum target/wasm32-wasip1/release/vortex_mod_youtube.wasm
sha256sum plugin.toml
```
Copier les deux hash SHA-256 pour l'étape suivante.

- [ ] **Step 3 : Tag + push le plugin**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
git tag -a v1.2.0 -m "Release v1.2.0 — add download_to_file for 1080p DASH support"
git push && git push --tags
```

- [ ] **Step 4 : Créer la GitHub Release**

```bash
cd /home/matvei/projets/vx/vortex-mod-youtube
gh release create v1.2.0 \
  target/wasm32-wasip1/release/vortex_mod_youtube.wasm \
  plugin.toml \
  --title "v1.2.0 — 1080p DASH support" \
  --notes "## What's new

### Added
- \`download_to_file\` plugin function: delegates DASH download + ffmpeg merge to
  yt-dlp, enabling true 1080p/1440p/2160p downloads from YouTube.

### Fixed
- Downloads silently downgrading to 360p when 1080p was requested but only DASH
  streams were available.

## Checksums
See \`plugin.toml\` included in this release for SHA-256 verification."
```

- [ ] **Step 5 : Mettre à jour `vortex/registry/registry.toml`**

Remplacer l'entrée `vortex-mod-youtube` existante par :

```toml
[[plugin]]
name                 = "vortex-mod-youtube"
description          = "YouTube video/playlist/shorts/channel downloader via yt-dlp"
author               = "vortex-community"
version              = "1.2.0"
category             = "crawler"
repository           = "https://github.com/mpiton/vortex-mod-youtube"
checksum_sha256      = "<SHA256_DU_WASM>"
checksum_sha256_toml = "<SHA256_DU_PLUGIN_TOML>"
official             = true
min_vortex_version   = "0.1.0"
```

Remplacer `<SHA256_DU_WASM>` et `<SHA256_DU_PLUGIN_TOML>` par les valeurs calculées à l'étape 2.

- [ ] **Step 6 : Commit registry**

```bash
cd /home/matvei/projets/vx/vortex
git add registry/registry.toml
git commit -m "chore(registry): bump vortex-mod-youtube to 1.2.0"
```

---

## Self-Review

### Couverture spec

| Exigence | Tâche |
|----------|-------|
| Plugin `download_to_file` WASM export | Tasks 1, 2 |
| yt-dlp DASH + merge ffmpeg | Task 1 (`yt_dlp_args_for_download_to_file`) |
| `DomainError::AdaptiveStreamOnly` | Task 4 |
| `PluginLoader::download_to_file` trait | Task 5 |
| `ExtismPluginLoader` implémentation | Task 6 |
| `RegisterLocalFileCommand` + handler | Task 7 |
| Fallback IPC `AdaptiveStreamOnly` | Task 8 |
| Release plugin v1.2.0 | Task 9 |
| Registry mis à jour | Task 9 |

### Vérifications de cohérence
- `DownloadedFileInfo` défini en Task 5, utilisé en Tasks 6 et 8 ✓
- `RegisterLocalFileCommand` défini en Task 7 (mod.rs), utilisé en Task 8 ✓
- `DomainError::AdaptiveStreamOnly` défini en Task 4, détecté en Task 6, matché en Task 8 ✓
- `parse_download_path_from_stdout` défini en Task 1, importé en Task 2 ✓
- `DEFAULT_DOWNLOAD_TIMEOUT_MS` défini en Task 1, utilisé en Task 2 ✓
