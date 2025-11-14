# Nitro Repo Architecture Overview

Nitro Repo is split into a Rust back end (multi-crate workspace) and a Vue/Vite front end.

## Crate Layout
- `nitro_repo/`: main binary crate. Hosts HTTP server (Axum), repository dispatch, authentication, OpenAPI, metrics.
- `crates/core`: shared domain types (database entities, repository metadata, project models, security utilities).
- `crates/storage`: abstraction over storage backends (local filesystem, S3). Provides `DynStorage` trait object used by repositories.
- `crates/macros`: derives like `DynRepositoryHandler`.

## Repository Abstractions
- `Repository` trait: defines HTTP handling (`handle_get`, `handle_put`, etc.), metadata (id, visibility, configs).
- `RepositoryType`: factory interface handling descriptor metadata, config validation, repository instantiation from DB.
- Dynamic dispatch via `DynRepository` enum produced by `DynRepositoryHandler` macro.
- Config descriptors (implementing `RepositoryConfigType`) supply schemars schemas, validation, defaults; registered in `REPOSITORY_CONFIG_TYPES`.

## Repository Implementations
- Maven: hosted + proxy support (`maven` module).
- NPM: hosted registry (`npm` module).
- Cargo: hosted registry with sparse index support (`cargo` module) exposing the Cargo publish API, sparse index files, and archive downloads.
- Python / PHP (Composer): hosted-only modules introduced during current iteration, leveraging shared `RepositoryExt` helpers, metadata inserts into `VersionData.extra`.

## HTTP Flow
- `repository_router` in `repo_http.rs`: resolves storage/repo by path, constructs `RepositoryRequest` (HTTP parts, body, parsed `StoragePath`, authentication).
- Repository-specific handler invoked via matching HTTP method.
- `RepoResponse` converts storage/file metadata or custom responses into Axum responses. Shared tracing instrumentation ties into `RepositoryRequestTracing`.

## Authentication
- Session cookies power the browser experience (`/api/user/login` manually verifies credentials and issues a 24h session).
- API tokens expose scoped automation access and authenticate via the `Authorization: Bearer <token>` header.
- Optional SSO support is exposed through `/api/user/sso/login`. When enabled (`SecuritySettings.sso`), Nitro Repo expects the upstream SSO proxy to forward identity headers (`X-Forwarded-User` by default) and can auto-provision users when `auto_create_users` is true. Administrators can update these settings at runtime via `/api/security/sso` (exposed in the Admin UI) and the values persist in the `application_settings` table.
- Docker Registry clients use the standard Bearer token flow: any write request that reaches `/v2/**` without credentials is answered with a `WWW-Authenticate: Bearer …` challenge whose `realm` points at `/v2/token` and whose `scope` reflects the concrete repository path resolved for the request. The `/v2/token` handler (in `repository::docker::auth`) inspects the incoming authentication (session, password, or long-lived API token), validates the requested repository scopes, and mints a short-lived repository token (`NewRepositoryToken`). The token is hashed at rest, carries its allowed actions, and is returned to the Docker client as the Bearer token that must accompany the retried request.

## Data Persistence
- DB layer via `nr_core::database::entities`. Projects and versions inserted/queried directly in repositories for metadata.
- Configs stored in `repository_configs` table; retrieved through `DBRepositoryConfig`.
- Repository lookup/registration handled by `NitroRepo` state with in-memory caches keyed by UUID and name pair.

## Front End Integration
- Vue components under `site/`: repository type configs (`types/<repo>/`), helper views, admin panel integration.
- `site/src/types/repository.ts`: registry of repository types/config components used in UI when creating/managing repositories.

## Build/Test Notes
- Rust workspace managed via Cargo; `cargo build` requires system `pkg-config` + OpenSSL headers (`libssl-dev`/`openssl-devel`).
- Front end built separately with Vite (not covered in this summary).
