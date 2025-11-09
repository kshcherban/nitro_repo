# Helm Repository Routes

Nitro Repo exposes a Helm repository through both the classic chart HTTP interface and the OCI Distribution API. The following reference summarises every supported route so operators and automation can target the correct endpoints.

Use the placeholders below when adapting the examples:

- `<host>` – Nitro Repo base host (e.g. `nitro.example.com`)
- `<storage>` – Storage name that owns the repository (e.g. `default`)
- `<repo>` – Helm repository name (e.g. `helm-charts`)
- `<chart>` – Chart name (e.g. `webapp`)
- `<version>` – Semantic version of the chart (e.g. `1.2.3`)
- `<token>` – Bearer token or HTTP basic credentials (Helm supports either)
- `<uuid>` – Upload UUID returned by the OCI blob upload flow
- `<digest>` – OCI descriptor digest (e.g. `sha256:deadbeef…`)

All paths below are rooted at:

```
https://<host>/repositories/<storage>/<repo>/
```

## HTTP Chart Repository

### Download Index
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/repositories/<storage>/<repo>/index.yaml"
```
- `?force=true` skips the server-side cache and re-renders the index.
- Response headers include `Content-Type: application/x-yaml`.

### Download Chart Archive
```bash
curl -OL \
     -H "Authorization: Bearer <token>" \
     "https://<host>/repositories/<storage>/<repo>/<chart>-<version>.tgz"
```
- `HEAD` is supported for existence checks.
- `Range` headers allow resumable downloads.
- Alias form: `/charts/<chart>-<version>.tgz`.

### Upload Chart Archive (HTTP)
```bash
curl -X PUT \
     -H "Authorization: Bearer <token>" \
     --upload-file "./<chart>-<version>.tgz" \
     "https://<host>/repositories/<storage>/<repo>/<chart>-<version>.tgz"
```
- Validates chart metadata and size constraints.
- Returns `201 Created` on first publish, `200 OK` on overwrite when allowed.

### Upload Provenance File
```bash
curl -X PUT \
     -H "Authorization: Bearer <token>" \
     --upload-file "./<chart>-<version>.tgz.prov" \
     "https://<host>/repositories/<storage>/<repo>/<chart>-<version>.tgz.prov"
```
- Associates `.prov` signatures with previously uploaded charts.

### List Stored Packages
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/repositories/<storage>/<repo>/packages"
```
- Returns JSON metadata (name, version, digest, provenance flag).

### Delete Packages
```bash
curl -X DELETE \
     -H "Authorization: Bearer <token>" \
     -H "Content-Type: application/json" \
     -d '{
           "charts": [
             { "name": "<chart>", "version": "<version>" }
           ]
         }' \
     "https://<host>/repositories/<storage>/<repo>/packages"
```
- Removes HTTP artefacts, provenance, OCI blobs/manifests, and database entries.

## ChartMuseum-Compatible API

### List Charts
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/repositories/<storage>/<repo>/api/charts"
```
- Response is a JSON object keyed by chart name, returning versions and metadata.

### List Versions for a Chart
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/repositories/<storage>/<repo>/api/charts/<chart>"
```

### Multipart Upload (ChartMuseum style)
```bash
curl -X POST \
     -H "Authorization: Bearer <token>" \
     -F "chart=@<chart>-<version>.tgz" \
     -F "prov=@<chart>-<version>.tgz.prov" \
     "https://<host>/repositories/<storage>/<repo>/api/charts"
```
- Accepts `chart` (required) and `prov` (optional) fields.

## OCI Registry Endpoints

Hybrid repositories mirror every HTTP upload into the OCI layout. OCI-only repositories expose only the following routes.

All OCI paths are nested under `/v2/<chart>/*` relative to the repository root. For clarity, the examples below expand the full URL.

### Registry Ping
```bash
curl "https://<host>/repositories/<storage>/<repo>/v2/"
```

### Initiate Blob Upload
```bash
curl -X POST \
     -H "Authorization: Bearer <token>" \
     -H "Content-Length: 0" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/blobs/uploads/"
```
- Response headers include `Location: .../<uuid>` and `Docker-Upload-UUID`.

### Upload Blob Chunk
```bash
curl -X PATCH \
     -H "Authorization: Bearer <token>" \
     -H "Content-Type: application/octet-stream" \
     --data-binary "@chunk.bin" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/blobs/uploads/<uuid>"
```
- Repeat as needed until all bytes are sent.

### Complete Blob Upload
```bash
curl -X PUT \
     -H "Authorization: Bearer <token>" \
     --data-binary "" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/blobs/uploads/<uuid>?digest=<digest>"
```
- Stores the blob under `blobs/<digest>` and responds with `201 Created`.

### Download Blob
```bash
curl -L \
     -H "Authorization: Bearer <token)" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/blobs/<digest>"
```

### Upload Manifest (Tag)
```bash
curl -X PUT \
     -H "Authorization: Bearer <token>" \
     -H "Content-Type: application/vnd.cncf.helm.chart.v1+json" \
     --data-binary "@manifest.json" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/manifests/<version>"
```
- Nitro automatically stores a digest copy at `/manifests/<digest>`.

### Fetch Manifest
```bash
curl -H "Authorization: Bearer <token>" \
     -H "Accept: application/vnd.cncf.helm.chart.v1+json" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/manifests/<reference>"
```
- `<reference>` can be a tag (e.g. `<version>`) or a digest.

### Delete Manifest
```bash
curl -X DELETE \
     -H "Authorization: Bearer <token>" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/manifests/<reference>"
```
- Removes associated HTTP artefacts when present in hybrid mode.

### Delete Blob
```bash
curl -X DELETE \
     -H "Authorization: Bearer <token)" \
     "https://<host>/repositories/<storage>/<repo>/v2/<chart>/blobs/<digest>"
```

## Authentication Notes

- HTTP basic auth and bearer tokens both work for the classic chart endpoints.
- OCI clients (Helm v3) should use `helm registry login` to obtain bearer tokens.
- Permissions:
  - `RepositoryActions::Read` → download/index access
  - `RepositoryActions::Write` → chart uploads, deletions, OCI pushes

With these routes, Helm v2 clients (`helm repo add`, `helm install`) and Helm v3 clients (`helm push`, `helm pull`, `helm install oci://…`) operate seamlessly against Nitro Repo.
