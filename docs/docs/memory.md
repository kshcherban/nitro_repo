# Project Knowledge Log

## Repository Architecture
- Nitro Repo hosts multiple repository types (Maven, Python, PHP, NPM) behind a unified admin experience.
- Repository configs are stored per repository (e.g. `repository_configs` rows keyed by type, such as `maven`, `python`, `auth`).
- Storage layout differs by repo type:
  - Proxies cache under `packages/` (hashed prefix) while hosted repos (e.g. Python) store packages directly under normalized project paths.
- Admin UI tabs are driven by repository type heuristics (Python/PHP/NPM/Maven) and fetch repository kind via `/api/repository/{id}/config/{type}`.

## Recent Changes (Codex Session)
- **Packages API**: Added strategy-aware listing and deletion to support Maven hosted/proxy and Python hosted layout. Hosted repos now list directly from root paths; proxies stay under `packages/`.
- **Admin Packages UI**: Added search, dynamic headers, and hosted/proxy messaging; extended visibility to Maven repos.
- **Maven Proxy**: Implemented upstream `HEAD` fallback so Maven clients probing uncached artifacts (e.g. `maven-resources-plugin`) receive accurate metadata instead of 404.
- **Artipie Compatibility**: Added `/api/artifact/...` and `/api/meta/...` redirectors so legacy Artipie API paths reach Nitro repositories without changing client configuration.
- **Python Hosted Simple Index**: Served PEP 503 simple index HTML pages (root and package-specific) including hashing and `data-requires-python` attributes.

## Operational Notes
- `./dev.sh` is a standard rebuild and app restart for debugging
- Maven proxy routes default to `https://repo.maven.apache.org/maven2/`; trimming trailing slash is handled by `ProxyURL` normalization.
- Use jaeger from @docker-compose.dev.yml to access traces and debug further
- Fetch trace from jaeger: `curl -s http://localhost:16686/api/traces/6411c391351280a02b171a4b9c624b56 | jq ...`
- Logs can be retrieved with `docker compose logs nitro_repo`
