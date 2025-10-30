## Python & PHP Artifact Support

- [x] **Specification & Design**: Define supported workflows (PyPI-style uploads, Composer metadata), auth expectations, and document flows for stakeholders.
- [x] **Data Model Extensions**: Update shared core types to capture Python wheel metadata and Composer package details in `VersionData.extra`, documenting schemas for downstream consumers.
- [x] **Backend Implementation**: Add `python` and `php` repository modules handling uploads, listings, downloads, and metadata extraction; ensure files persist via `nr_storage` and responses mirror client expectations.
- [x] **Configuration & Registration**: Create config enums (hosted/proxy as needed), register new types in repository registries, expose OpenAPI metadata, and validate configuration transitions.
- [x] **Frontend & UX**: Extend Vue admin components to surface new repository types, helpers, and icons; ensure repository pages render Python/PHP metadata.
- [x] **Quality & Documentation**: Add focused unit tests, update feature matrix, create language-specific docs, and outline verification steps for pip/composer clients.
