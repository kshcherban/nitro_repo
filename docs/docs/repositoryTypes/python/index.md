# Python Repository

Nitro Repo provides a simple Python artifact store for wheels and source distributions.

## Uploading Packages

- Authenticate with an account that has `Write` permissions for the repository.
- Upload to `/repositories/<storage>/<repository>/<package>/<version>/<filename>` using `PUT` or `POST`.
- Nitro Repo records metadata (package name, version, and filename) for search and project pages.

## Downloading Packages

Published files are available under the same path used when uploading. Any authenticated or public
client with read access can fetch the artifacts directly.

## Proxy Mode

- Switch the repository configuration to **Proxy** and add one or more upstream URLs (for example
  `https://pypi.org`). Nitro Repo appends the request path automatically, so you do not need the
  `/simple` suffix in the upstream.
- Nitro Repo will fetch packages on demand. Binary artifacts are cached locally on first request;
  directory-style HTML responses are streamed directly from upstream.
- If the repository is private, readers still need permission—Nitro Repo only reaches out to the
  upstream after the local permission check succeeds.

## Metadata

Package metadata is stored in `project_versions.extra` as a `PythonPackageMetadata` object, making
it available to the user interface and APIs for downstream consumption.

## Usage with uv

How to exercise with uv:

1. Hosted upload – configure a hosted Python repo, create a user with write access, then publish from a project directory:

```
uv publish --index-url https://<host>/repositories/<storage>/<repo> \
            --username <nitro_user> --password <nitro_password>
```

2. Hosted download – install a specific wheel from Nitro Repo directly:

```
uv pip install https://<host>/repositories/<storage>/<repo>/<package>/<version>/<filename>
```
 (replace <filename> with the actual wheel or sdist you uploaded.)

3. Proxy download – flip the repo to Proxy mode with an upstream such as https://pypi.org, then:

```
uv pip install --index-url https://<host>/repositories/<storage>/<repo>/simple <package>
```

 Nitro Repo will fetch from PyPI on first access and cache locally.

4. Proxy verification – repeat the install (step 3) and observe that the second run is served from Nitro Repo without contacting the upstream.

For hosted NPM repos, the proxy/hosted flows follow the same pattern via the updated Vue config screen.
