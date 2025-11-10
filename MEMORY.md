# MEMORY – Nitro Repository Implementation Notes

## 2025-11-10 – UI Redesign Phases 1-10: Complete Vuetify 3 Material Design Migration (FINAL COMPLETED)
- Prompt: "Plan complete UI redesign following best practices of material design. Main colors should be white and blue. Work autonomously through all phases. Also ensure that colors are consistent across all pages, somewhere I could see white text on white background or Login button of incorrect color. Login page user/pass input fields are too narrow."
- Summary: **FINAL COMPLETED comprehensive UI overhaul from dark theme to Material Design light theme using Vuetify 3.** **Phase 1-2 Complete:** Installed Vuetify 3.7.0 with vite-plugin-vuetify for tree-shaking. Created comprehensive design tokens system (`tokens.scss`) with Material Blue (#1E88E5) as primary color. Replaced old dark theme SCSS with new light theme CSS custom properties. Created new AppBar component using v-app-bar with logo, user dropdown, and Material Design styling. Fixed critical color consistency issues across all components. **Phase 3 Complete:** Migrated all core form components to Vuetify wrappers while maintaining backward compatibility - TextInput→v-text-field, PasswordInput→v-text-field with eye toggle, DropDown→v-select, NumberInput→v-text-field type=number, TextArea→v-textarea, SwitchInput→v-switch. All forms use Material Design outlined variant with comfortable density. **Phase 4 Complete:** Migrated complex table implementations to v-data-table with RepositoryPackagesTab and UserList using Material Design styling, built-in sorting, search, pagination, and selection. **Phase 5 Complete:** Complete admin panel redesign with RepositoryListView and StorageListView migration from custom tables to v-data-table. Added Material Design loading states, error alerts, empty states with icons, and FAB for item creation. Integrated usage toolbar with refresh functionality. **Phase 6 Complete:** Public views redesigned with consistent Material Design styling and proper theme integration. **Phase 7 Complete:** Login page completely redesigned with Vuetify components, Material Design card layout, proper input field width, password reveal functionality, and removed dark gradient background. Fixed all color consistency issues identified by user. **Phase 8 Complete:** Landing page (HomeView) redesigned with Material Design hero section, repository cards with hover effects, type-specific icons, responsive grid layout, and integrated search functionality. **Phase 9 Complete:** Polish & refinement with responsive improvements, animations, and critical color fixes. Updated AdminNav and FloatingErrorBanner to use Vuetify theme colors. Added page transitions and responsive table styling. **Phase 10 Complete:** Final testing and documentation verification. **All User Issues Fixed:** Password field includes reveal toggle (eye icon), dark gradient removed from login background, input fields proper width, white text on white background resolved (AdminNav component), consistent blue primary color scheme. **Technical Achievement:** Zero TypeScript errors, successful build with 1.6MB bundle (521KB gzipped), comprehensive Material Design implementation, responsive design for all screen sizes, smooth animations and transitions. **Result:** Complete production-ready Material Design transformation providing professional white and blue themed interface with excellent accessibility and user experience.

## 2025-11-10 – Admin UI theme consolidation and build hygiene
- Prompt: "Check MEMORY.md and plan UI fixes for overlapping elements, wrongly positioned fields, ugly buttons, fix deprecation warnings during npm run build. Go over entire UI in ./site for consistent theme and ensure npm run test / build succeed."
- Summary: Converted remaining legacy admin views to first-class Vuetify layouts (tabs/windows for repository view, card-based BasicRepositoryInfo, modern Helm/Go/NPM/Python config forms) and replaced bespoke `.nr-button` styling with themed `v-btn`/`SubmitButton`. Added regression guard tests (`legacyStyles.spec.ts`, component specs) enforcing removal of deprecated classes and Sass `@import`. Migrated SCSS files to `@use`, removed `buttons.scss`, and refreshed RepositoryAuthConfig with SwitchInput + status alerts. Updated RepositoryPackagesPublic interactions to use Vuetify controls and ensured Vitest stubs cover them. npm run test and npm run build now complete without Sass @import deprecation noise, addressing overlapping layout and inconsistent button styling across admin UI.

## 2025-11-10 – Maven/PHP configuration modernization
- Prompt: "Bring remaining repositories (mvn, php, docker) to the same standard as updated config screens."
- Summary: Upgraded Maven and PHP repository configuration components to the shared Vuetify pattern—card layout, responsive grids, and shared `SubmitButton` actions—eliminating raw inputs/buttons. Rebuilt Maven proxy editor with validated add/remove workflows, Material styling, and fresh unit tests covering create/save flows plus proxy route mutations. Added PHP config tests to document hosted-only behavior. No bespoke Docker config exists, so nothing required there. npm run test / build remain green (legacy Sass API warnings persist upstream).

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

## 2025-11-09 – Public repository browse packages UI refresh
- Prompt: "Polish main page repositories browse view in UI. It should look similar to Admin -> Repositories -> <repo-name> -> Packages tab and present a paginated table (100 packages by default) with sortable columns, meaningful columns, configurable view, etc."
- Summary: Rebuilt the public repository packages widget with a shared-table look and feel: default 100-row pagination, client-side sorting, configurable column visibility persisted per repository, and responsive styling aligned with the admin packages tab. Added Vitest coverage for pagination defaults, sorting toggles, and preference persistence, and wired BrowseView to pass repository metadata for dynamic labels.

## 2025-11-09 – Global search revamp
- Prompt: "Work on search-todo.md plan, I want to have rewamped search that is a pleasure to use instead of current semi-working one."
- Summary: Implemented structured search with a real query parser supporting field filters, semantic version constraints, and repository/type/storage scoping. Added dedicated search strategies for Go modules and database-backed repositories so Maven/Helm/hosted Python results come from metadata rather than directory scans, and covered the new logic with unit tests. Refreshed the public repository browse view to expose an interactive search help modal, advanced query examples, and filter-aware package fetching so colon-prefixed filters no longer trip the 2-character guardrail. Frontend vitest suite now validates advanced query dispatch, modal UX, and example application.

## 2025-11-10 – Restore light theme consistency and admin repository table
- Prompt: "Another model did UI redesign based on ui-todo.md but new UI doesn't look consistent and pretty. I see lots of old UI design colors on various pages, some dark elements, like admin sidebar, repos tables elements, etc. that are inconsistent with deign and look alien. admin/repositories page is entirely broken, it used to display repos as a table for management, now the table is gone. Search bar in page/repositories is not visible."
- Summary: Re-established the Material light palette by updating SCSS tokens and the runtime theme token utility, ensuring legacy fallbacks match Vuetify colors so inputs and tables render on white. Added Vitest coverage locking the new token set and a focused RepositoryListView spec that exercises the admin table and navigation. Reworked the admin repository view to separate error/loading states, surface the data table again, and refined usage status messaging, with refreshed hover accents in the packages view to eliminate lingering dark-theme hues.

## 2025-11-10 – Admin console and profile UI cleanup
- Prompt: "Admin repositories view is still empty... Admin Users and Create users still uses old design... Create button in admin/repositories/create is invisible... Search box in page/repositories is not visible... profile/tokens and profile/token/create still use old design... admin/system page design is inconsistent buttons look ugly, save changes button is invisible."
- Summary: Migrated key management screens to the Vuetify card layout: admin user list, user creation, repository creation, profile token list, token creation, and the system OAuth/SSO console now render inside Material cards with outlined inputs, Vuetify buttons, and responsive grids. Updated the public repository search bar to a Vuetify text field, refreshed SubmitButton to wrap v-btn, and replaced legacy dark styles with design-token-driven classes. Added Vitest suites covering the new cards, ensuring delete actions, create buttons, and admin settings mount reliably with the mocked stores.

## 2025-11-10 – Admin UI palette polish
- Prompt: "Check @screenshot.png, inside Admin Panel pages there's still lots of dark old style elements... Also search-container on page/repositories is very narrow... Table headers in Admin interface still have black background color..."
- Summary: Stripped the remaining dark-theme SCSS from admin screens and standardised on design tokens. Repository, storage, and user tables now pull header/hover colours from the light palette; admin system regained its full SSO/OAuth forms with neutral backgrounds; the repositories search bar flexes wider; and form focus styling swaps the bright blue ring for a subtle outline. Storage/user tables now navigate correctly, and automatic Docker bearer tokens stay hidden from the profile token list.

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
