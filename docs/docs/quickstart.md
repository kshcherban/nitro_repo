# Quickstart: Run Nitro Repo with Docker

These steps launch Nitro Repo and a supporting PostgreSQL instance using Docker Compose. All state is
stored in Docker volumes so the data survives container restarts.

## 1. Prerequisites

- Docker Engine + Compose plugin.
- Open ports 5432 (PostgreSQL) and 6742 (Nitro Repo UI/API) on your machine.

Clone the repository and move into it:

```bash
git clone https://github.com/wherkamp/nitro_repo.git
cd nitro_repo
```

## 2. Build the container image

```bash
docker compose build
```

This compiles the Rust backend and packages the pre-built Vue frontend into the binary.

The compose stack mounts `docker/config/nitro_repo.toml` into the container at
`/data/nitro_repo.toml`. Edit this file before starting the services if you want to change default
database credentials, storage paths, or session lifetimes.

## 3. Start PostgreSQL

```bash
docker compose up -d postgres
```

Wait for the container healthcheck to report healthy:

```bash
docker compose ps
```

## 4. Run the installer

Execute the one-time interactive installation inside the Nitro Repo container. This creates the
first admin user, configures storage, and writes defaults into the database.

```bash
docker compose run --rm nitro_repo --install
```

Follow the prompts to supply:

- Admin username/password.
- Default storage root (the container mounts `/data`; the recommended value shown in the wizard is fine).
- Any optional settings you wish to override.

When the installer finishes, exit the container (it terminates automatically).

## 5. Launch Nitro Repo

```bash
docker compose up -d nitro_repo
```

The server listens on `http://localhost:6742`. Log in with the admin credentials you created during
the install step, then configure additional storages or repositories via the UI.

To tail logs:

```bash
docker compose logs -f nitro_repo
```

## 6. Stopping the stack

```bash
docker compose down
```

The volumes `nitro_repo_data` (application data) and `postgres_data` (database) are preserved. Remove
them explicitly if you want a fresh install:

```bash
docker compose down -v
```
