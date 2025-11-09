# MEMORY – Go Repository Implementation Notes

## Hosted Repository Upload Flow
- Uses a multipart `POST /upload` endpoint (Athens compatible) with fields:
  - `module` (zip archive), `version`, `module_name`, optional `info` and `gomod`.
- Requires `RepositoryActions::Write`; responds with 201 + JSON summary.
- Auto-generates `.info` when missing (`Version`, `Time`).
- Extracts or validates `go.mod`; rejects archives missing it.
- Normalises uploaded zip:
  - Paths must not include `..` or absolute prefixes.
  - Repackages to `<module>@<version>/...` layout.
  - Writes canonical zip via `GoRepositoryExt::save_go_module_file`.
- After saving artefacts, refreshes:
  - `@v/list` (sorted unique versions),
  - `@latest` and `go.mod` aliases when both `.info` and `.zip` exist.
- PUT uploads to individual `/@v/{version}` artefacts remain supported.

## Proxy Cache Listing
- `gather_package_dirs` now returns `(display_name, storage_relative)` tuples.
- For Go proxy caches, strips trailing `/@v` when building display names.
- The Packages admin UI uses the tuple to show clean module labels and allows delete.

## SumDB Behaviour
- Proxy repositories forward sumdb requests to `https://sum.golang.org`.
- Hosted repositories return:
  - `/sumdb/sum.golang.org/supported` → `false`
  - `/sumdb/.../lookup` and `/tile/...` → `501`.
- Fetching private hosted modules requires setting `GONOSUMDB`, `GOPRIVATE`, etc., or `GOSUMDB=off`.

## Artipie / Artifactory Routes
- `/api/{repo}/upload` forwards to canonical `/repositories/<storage>/<repo>/upload` (307 redirect).
- Planned `/artifactory/{repo}/...` alias will resolve repositories by name (first match) and rewrite to canonical paths.

## 2025-11-08 – Helm repository support
- Prompt: "Based on helm-todo.md implement helm v2, v3 and OCI helm repo support in nitro."
- Summary: Added Helm hosted backend with hybrid HTTP↔OCI publishing, persisted chart metadata (including provenance and OCI digests), implemented package deletion cleanup, created Vue admin config for Helm repositories, updated docs, and verified Rust/Vue builds.

## 2025-11-09 – Helm Chart for Nitro Repo deployment
- Prompt: "In examples/helm create me helm chart for hosting nitro repo (this project), it should install same components as docker-compose.yml and docker-compose.dev.yml do. For postgres and jeager use their official postgres helm charts as dependencies. Do not overengineer, ensure that all settings to pods are done via env variables."
- Summary: Created comprehensive Helm chart for deploying Nitro Repo with PostgreSQL and Jaeger dependencies. Chart includes proper security contexts, resource limits, health checks, ingress support, and documentation. Supports both development (with Jaeger tracing) and production configurations. Chart successfully packaged and validated for deployment to Kubernetes clusters. Successfully uploaded test chart to nitro.sudoers.dev/test/helm repository using ChartMuseum API with Bearer token authentication. Chart is accessible via: https://nitro.sudoers.dev/repositories/test/helm/index.yaml

## 2025-11-09 – Helm Chart Security Hardening
- Prompt: "I also noticed that you allow passing passwords directly from values, that is considered unsecure and a bad practice. Allow setting postgres url with password from env instead..."
- Summary: Hardened Helm chart security by removing hardcoded passwords from ConfigMap and implementing environment variable support with Kubernetes secret references. Added `env` parameter for custom environment variables with secretKeyRef support, updated external database configuration to use secure secret references, removed adminPassword from values.yaml, and documented security best practices. Chart now follows security best practices by never storing sensitive data in ConfigMaps or values.yaml.

## 2025-11-09 – Helm repository hardening & admin UX
- Prompt: "Create helm routes document in docs/docs/repositoryTypes/helm/routes.md similar how it's done for other repos types." (Follow-up directives covered OCI push issues, admin UI feedback, and error styling.)
- Summary: Hardened Helm support by accepting repeated `scope` query parameters, normalising `/repositories/...` OCI paths, and processing manifest uploads to persist chart metadata for packages tab visibility. Added DB-backed package listing/deletion for Helm, documented HTTP/OCI routes, and refreshed admin UI (floating error banners, left-aligned create forms, consistent password rules) per Material Design guidance. Verified with `cargo test` and `npm run build`.

## 2025-11-09 – Helm chart push recovery & admin UX save flow
- Prompt: "Check MEMORY.md docs/docs/repositoryTypes/helm/index.md ... Fix SQL 42601 on helm manifest push, make overwrite toggle persist, and align Helm config Save button styling. Verify with ./dev.sh then helm push."
- Summary: Added regression tests for project version updates and Helm runtime state, fixed SQL update builder to filter by version id, introduced reloadable Helm runtime state, implemented repository reload to apply new configs, refreshed Helm admin form styles, and added Vitest coverage ensuring save triggers reload. Verified end-to-end with `./dev.sh`, `helm push`, cargo unit tests, and vitest.

## Nitro Repo Platform Notes

- **Async/Blocking rules**
  - Nitro’s main HTTP handlers run on Tokio; avoid blocking work.
  - Any CPU/IO heavy synchronous code must run via `tokio::task::spawn_blocking` or equivalent helper.
  - Storage access helpers (`nr_storage`) expose async APIs that internally use blocking thread pools; call them directly from async contexts.
  - When adding new heavy tasks, prefer `spawn` or background jobs outside request path.

- **Database access**
  - Uses `sqlx` with async connection pools (`site.database`).
  - Repository-specific fetches (e.g., loading configs) rely on typed helpers; follow existing patterns (e.g. `DBRepositoryConfig::<T>::get_config`).
  - Wrap DB errors in repository-specific error enums implementing `IntoErrorResponse`.
  - Ensure transactions are short-lived; long-running operations should cache results or precompute.

- **Storages**
  - `DynStorage` abstracts local, S3, etc. Always operate through async methods (`open_file`, `save_file`, `delete_file`).
  - When manipulating contents, stream or use helper conveniences (`StorageFile::read_to_vec`), then persist via `FileContent`.
  - Keep paths predictable; many upstream clients rely on specific directory semantics.

- **Repositories**
  - Implement `Repository` trait; register new types via `GoRepositoryType`-style enums.
  - Enforce permission checks using `RepositoryRequest.authentication` helpers (`get_user_if_has_action`).
  - Use `RepoResponse` helpers to return standard responses (e.g., `basic_text_response`, `unsupported_method_response`).

- **Frontend / Admin UI**
  - Vue components live under `site/src/components`. New repository types require form components (`GoConfig.vue`, `GoProjectHelper.vue`) and metadata registration in `repository.ts`.
  - Packages tab consumes `/api/repository/{id}/packages`; ensure backend responses provide clean labels for search and delete flows.
  - Any new API endpoints should include OpenAPI annotations (`utoipa`) to stay discoverable.

- **Testing**
  - Use `cargo check` and unit tests for backend Rust code; keep new helpers small and testable (e.g., `longest_common_prefix`).
  - For frontend, update docs/examples to reflect API semantics; manual smoke tests useful for env vars (GOPROXY, GOPRIVATE).
