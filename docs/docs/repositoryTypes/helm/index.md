# Helm Repositories

Nitro Repo delivers first-class Helm repository hosting with support for both classic HTTP chart clients and the OCI-based workflows introduced in Helm v3. Create a Helm repository from the Admin UI and choose the repository mode that matches your deployment:

- **Hybrid (default)** – exposes HTTP endpoints (`index.yaml`, chart downloads) and an OCI registry simultaneously. Uploading a chart via either protocol keeps both surfaces in sync.
- **HTTP** – behaves like a traditional chart repository (`helm repo add`, `helm install`).
- **OCI** – exposes only the distribution-spec API (`helm push`, `helm pull`).

## Configuration

| Setting | Description |
| ------- | ----------- |
| `Allow Overwrite` | Permits overwriting an existing chart version. Disabled by default. |
| `Public Base URL` | Optional fully qualified URL advertised inside `index.yaml`. Useful when Nitro sits behind a reverse proxy. |
| `Index Cache TTL` | Server-side cache duration (seconds) for rendered `index.yaml`. Use `0`/blank to disable caching. |
| `Max Chart Size` | Rejects uploads larger than the configured number of bytes. Defaults to 10&nbsp;MiB. |
| `Max Files Per Chart` | Caps the number of files allowed inside a chart archive. Defaults to 1,024. |

## Uploading Charts

### HTTP (ChartMuseum compatible)

```bash
helm package ./charts/webapp
curl -u token:secret \
  -T webapp-1.0.0.tgz \
  https://nitro.example.com/repositories/default/helm/webapp-1.0.0.tgz
```

### OCI

```bash
helm registry login nitro.example.com --username token --password secret
helm push webapp-1.0.0.tgz oci://nitro.example.com/repositories/default/helm/webapp
```

Hybrid repositories automatically mirror uploads between the HTTP and OCI layouts, keeping package metadata and the rendered index synchronized.

## Further Reading

- [HTTP & OCI Route Reference](./routes.md) – detailed list of every Helm endpoint exposed by Nitro Repo.
