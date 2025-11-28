# nitro_repo [![Documentation](https://img.shields.io/static/v1?label=nitro-repo.kingtux.dev&message=Here&style=for-the-badge&color=green)](https://nitro-repo.kingtux.dev/) [![Powered By Actix](https://img.shields.io/badge/Powered%20By-Actix-red?style=for-the-badge&logo=rust)](https://github.com/actix/actix-web)

[![issues](https://img.shields.io/github/issues/wherkamp/nitro_repo/help%20wanted)](https://github.com/wherkamp/nitro_repo/issues)

Nitro Repo is an open source free artifact manager. Written with a Rust back end and a Vue front end to create a fast
and modern experience.

### History

After years of using Nexus and then a bit of time of using StrongBox I decided I should design my own Artifact Manager
to create a fast and modern experience.

### Technical Design

- Backend or the heart of nitro_repo
  - SQLX for Postgres
  - Axum for HTTP Server
- Frontend
  - Vue
  - Vite

### Crates
- crates/core
  - Lays out some shared data types between different modules.
- crates/macros
  - Macros used by the other crates. To prevent writing so much code
- crates/storages
  - This layer provides different ways storing the artifacts that nitro-repo hosts

### Development

#### Prerequisites
- Docker and Docker Compose
- Rust (latest stable)
- Node.js (for frontend development)

#### Quick Start
1. Clone the repository
2. Run `./dev.sh` to build and start the development environment
3. Access Nitro Repo at `http://localhost:8000`
4. Access the API documentation at `http://localhost:8000/api/docs`

#### Tracing & Observability

The development environment includes distributed tracing with Jaeger to help diagnose performance issues:

- **Jaeger UI**: Available at `http://localhost:16686`
- **Tracing Configuration**: Automatically enabled in development via `docker-compose.dev.yml`
- **Key Traced Operations**:
  - HTTP requests (method, route, status code, timing)
  - Docker Registry V2 operations (blob upload, chunk processing)
  - Database operations and configuration loading
  - Authentication and session management
  - Background tasks and cleanup operations

#### Environment Variables
The development compose file automatically configures tracing with:
- `OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:4317`
- `OTEL_SERVICE_NAME=nitro`
- `NITRO_TRACING_ENABLED=true`

#### Troubleshooting
- If Docker upload operations are blocking the async runtime, check Jaeger traces for long-running spans
- Use `docker-compose logs nitro` to view application logs
- Restart services with `./dev.sh` after making configuration changes

