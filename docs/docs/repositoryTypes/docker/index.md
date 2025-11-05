# Docker Repository

Docker Repositories implement the Docker Registry HTTP API V2 specification, providing OCI-compliant container image storage and distribution.

## Repository Modes

Currently, Docker repositories support:

- **Hosted** - The repository is hosted on the server and is used to store Docker/OCI container images. This is used to create a private Docker registry.

Proxy mode for Docker registries is planned for future releases.

## Docker Registry API V2

Nitro Repo implements the Docker Registry HTTP API V2 specification, which is compatible with:

- Docker CLI (`docker push`, `docker pull`)
- Podman
- Containerd
- Any OCI-compliant container runtime

## Supported Image Formats

The Docker repository supports multiple manifest formats:

- **Docker Image Manifest V2, Schema 2** - Standard Docker image format
- **OCI Image Manifest** - Open Container Initiative image format
- **OCI Image Index** - Multi-platform image manifests (manifest lists)

## URL Structure

Unlike other repository types, Docker repositories use a special URL structure to maintain compatibility with Docker clients:

```
https://your-registry.com/{storage}/{repository}/{image-name}:{tag}
```

For example:
```
docker push repo.sudoers.dev/docker/docker-test/my-app:latest
```

Where:
- `docker` - Storage name
- `docker-test` - Repository name
- `my-app` - Image name
- `latest` - Image tag

## Quick Start

### 1. Create a Docker Repository

Create a new Docker repository through the Nitro Repo web interface or API.

### 2. Authenticate

```bash
docker login your-registry.com
Username: your_username
Password: your_password_or_token
```

### 3. Tag Your Image

```bash
docker tag my-app:latest your-registry.com/storage/repository/my-app:latest
```

### 4. Push Image

```bash
docker push your-registry.com/storage/repository/my-app:latest
```

### 5. Pull Image

```bash
docker pull your-registry.com/storage/repository/my-app:latest
```

## Browsing and Management

- The repository browser flattens the internal `v2/.../manifests` layout so you can navigate storages, repositories, and image names without seeing implementation folders. Selecting an image shows all uploaded tags as individual entries.
- The **Admin → Packages** tab lists Docker image manifests with the same paginated view used for other repository types. Administrators can select one or more tags and delete their manifests directly from the UI.
- Nitro Repo's global search now indexes Docker repositories. Queries match both the repository path (for example `library/nginx`) and individual tags (`latest`, build numbers, digests), returning the underlying manifest metadata.

Deleting a manifest removes the tag immediately. Blobs referenced by other manifests are preserved; garbage collection for unused blobs is handled separately.

## Authentication

Docker repositories support multiple authentication methods:

- **Username + Password** - Standard user authentication
- **Username + Token** - Use an auth token as the password (any username works)
- **Session-based** - For web UI access

See the [Authentication](#authentication-1) section in the standard documentation for more details.
