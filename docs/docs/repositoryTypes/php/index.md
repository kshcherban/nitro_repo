# Composer Repository

Nitro Repo can host Composer packages, providing a lightweight alternative to Packagist for
private or internal artifacts.

## Uploading Packages

- Authenticate with an account that has `Write` permissions for the target repository.
- Upload archives to `/repositories/<storage>/<repository>/<vendor>/<package>/<version>/<filename>`
  via `PUT` or `POST`.
- Nitro Repo automatically indexes the upload by `vendor/package` and version.

## Downloading Packages

Clients can access packages using the same path structure. Additional metadata is recorded in the
project database and surfaced through the Nitro Repo user interface.

## Metadata

Composer metadata is persisted to `VersionData.extra` as a `PhpPackageMetadata` document so that
downstream consumers can reason about uploaded artifacts.
