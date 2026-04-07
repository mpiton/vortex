# Task 04: Domain Ports

## Context
Les ports sont les traits Rust que le domaine definit. Les adapters les implementent. C'est le contrat entre le coeur metier et le monde exterieur. Les driving ports definissent comment on entre dans l'application (commands/queries). Les driven ports definissent ce dont le domaine a besoin du monde exterieur (persistence, reseau, filesystem...).

## Scope
- Driven ports (secondary) : tous les traits pour les sorties
- Driving ports (primary) : traits CommandHandler et QueryHandler
- Types associes pour les parametres et retours

## Implementation Details

### Files to Create/Modify
- `src-tauri/src/domain/ports/mod.rs`
- `src-tauri/src/domain/ports/driven/mod.rs`
- `src-tauri/src/domain/ports/driven/download_repository.rs` — Write repo
- `src-tauri/src/domain/ports/driven/download_read_repository.rs` — Read repo (CQRS)
- `src-tauri/src/domain/ports/driven/history_repository.rs`
- `src-tauri/src/domain/ports/driven/stats_repository.rs`
- `src-tauri/src/domain/ports/driven/plugin_loader.rs`
- `src-tauri/src/domain/ports/driven/credential_store.rs`
- `src-tauri/src/domain/ports/driven/file_storage.rs`
- `src-tauri/src/domain/ports/driven/http_client.rs`
- `src-tauri/src/domain/ports/driven/download_engine.rs`
- `src-tauri/src/domain/ports/driven/event_bus.rs`
- `src-tauri/src/domain/ports/driven/clipboard_observer.rs`
- `src-tauri/src/domain/ports/driven/config_store.rs`
- `src-tauri/src/domain/ports/driving/mod.rs`
- `src-tauri/src/domain/ports/driving/command_handler.rs`
- `src-tauri/src/domain/ports/driving/query_handler.rs`

### Key Functionality

**Driven Ports (ce que le domaine demande au monde exterieur) :**

```rust
trait DownloadRepository: Send + Sync {
    fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>>;
    fn save(&self, download: &Download) -> Result<()>;
    fn delete(&self, id: DownloadId) -> Result<()>;
    fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>>;
}

trait DownloadReadRepository: Send + Sync {
    fn find_downloads_with_progress(...) -> Result<Vec<DownloadView>>;
    fn find_download_detail(id) -> Result<Option<DownloadDetailView>>;
    fn count_by_state() -> Result<HashMap<DownloadState, usize>>;
}

trait PluginLoader: Send + Sync {
    fn load(manifest: &PluginManifest) -> Result<()>;
    fn unload(name: &str) -> Result<()>;
    fn resolve_url(url: &str) -> Result<Option<PluginInfo>>;
    fn list_loaded() -> Vec<PluginInfo>;
}

trait DownloadEngine: Send + Sync {
    fn start(download: &Download) -> Result<()>;
    fn pause(id: DownloadId) -> Result<()>;
    fn cancel(id: DownloadId) -> Result<()>;
}

trait EventBus: Send + Sync {
    fn publish(event: DomainEvent);
    fn subscribe(handler: Box<dyn Fn(&DomainEvent) + Send + Sync>);
}

trait FileStorage: Send + Sync {
    fn create_file(path: &Path, size: u64) -> Result<()>;
    fn write_segment(path: &Path, offset: u64, data: &[u8]) -> Result<()>;
    fn read_meta(path: &Path) -> Result<Option<DownloadMeta>>;
    fn write_meta(path: &Path, meta: &DownloadMeta) -> Result<()>;
    fn delete_meta(path: &Path) -> Result<()>;
}

trait HttpClient: Send + Sync {
    fn head(url: &str) -> Result<HttpResponse>;
    fn get_range(url: &str, start: u64, end: u64) -> Result<ByteStream>;
    fn supports_range(url: &str) -> Result<bool>;
}

trait CredentialStore: Send + Sync {
    fn get(service: &str) -> Result<Option<Credential>>;
    fn store(service: &str, credential: &Credential) -> Result<()>;
    fn delete(service: &str) -> Result<()>;
}

trait ConfigStore: Send + Sync {
    fn get_config() -> Result<AppConfig>;
    fn update_config(patch: ConfigPatch) -> Result<AppConfig>;
}
```

**Driving Ports (comment le monde exterieur entre dans l'application) :**

```rust
trait CommandHandler<C: Command> {
    type Output;
    async fn handle(&self, cmd: C) -> Result<Self::Output, AppError>;
}

trait QueryHandler<Q: Query> {
    type Output;
    async fn handle(&self, query: Q) -> Result<Self::Output, AppError>;
}
```

### Technologies Used
- Rust `std` uniquement — memes regles que le domaine
- `Send + Sync` bounds pour compatibilite async

### Architectural Patterns
- Ports & Adapters : le domaine definit les interfaces, les adapters implementent
- CQRS : separation write repo / read repo
- Dependency Inversion : le domaine ne connait que les traits, jamais les implementations

## Success Criteria
- [x] Tous les traits compiles sans erreur
- [x] Aucun `use` vers un crate externe dans `domain/ports/`
- [x] `Send + Sync` sur tous les traits (pour usage async)
- [x] Write repo (`DownloadRepository`) manipule des entites domaine
- [x] Read repo (`DownloadReadRepository`) retourne des view types (DTOs declares dans le domaine ou l'application)
- [x] Chaque port a une documentation Rust (`///`) decrivant sa responsabilite

## Testing & Validation

### Tests a ecrire
Les ports eux-memes ne sont pas testes (ce sont des traits). Mais verifier :
- Compilation reussie
- Les types des methodes sont coerents avec les domain models de Task 03
- Un mock in-memory compile correctement pour chaque trait

### Edge Cases
- `DownloadReadRepository` ne doit PAS avoir de methode `save()` (separation CQRS)
- `PluginLoader` doit gerer le cas ou un plugin n'est pas trouve

## Dependencies

**Must complete first**: Task 03 (Domain Models)

**Blocks**: Tasks 05-15 (tout ce qui implemente ou utilise les ports)

## Related Documentation
- **PRD**: §2.4 Module API (traits), §7.1 Moteur core
- **ARCHI**: Hexagonal Architecture > Secondary Ports, CQRS > Separation Read/Write Repositories

---
**Estimated Time**: 2 hours
**Phase**: Foundation
