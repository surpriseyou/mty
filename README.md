# MTY Package Manager

MTY is a small yum-like package management system for internal tools.

- `cli/`: cross-platform Rust CLI (`mty`)
- `server/`: ASP.NET Core API for package metadata, upload, download, and administration
- `web/`: Vue 3 + Vite + Element Plus admin console
- `docs/`: package format and API notes

## Quick Start

### Server

```powershell
cd server
dotnet run
```

The API stores package metadata in PostgreSQL and package files under `server/packages` by default. Development defaults use `Host=localhost;Port=5432;Database=mty;Username=postgres;Password=123`.

### Docker Compose

`docker-compose.yml` runs only the API and web admin console. It expects PostgreSQL to already exist.

```powershell
docker compose up --build
```

When using `podman-compose` on Windows, prefer detached mode:

```powershell
podman-compose up -d --build
podman-compose logs -f
```

Foreground `podman-compose up` can fail on Windows with `NotImplementedError` from Python's `asyncio.add_signal_handler`. Detached mode avoids that code path.

Default exposed ports:

- API: `http://localhost:5000`
- Web: `http://localhost:5173`

The web container serves the built Vue app with `caddy:latest`.

For a single image and container, use `docker-compose.fullstack.yml`. Run the publish script from PowerShell; it defaults to the full-stack image. Pass `-ImageMode Separate` only when publishing the original two images:

```powershell
.\scripts\publish-docker-hub.ps1
```

The script publishes `mty_app:latest` and a version tag, then removes those local tags. In the NAS Compose editor, set `DOCKERHUB_USER` to your Docker Hub username or replace the `YOUR_DOCKERHUB_USERNAME` placeholder in the image name. The combined container serves both the admin page and API on the existing API and web host ports. Stop the old two-container stack before switching because the host ports are reused; the package volume remains mounted at the same path.

Override the external database connection with:

```powershell
$env:POSTGRES_CONNECTION_STRING='Host=host.docker.internal;Port=5432;Database=mty;Username=postgres;Password=123'
docker compose up --build
```

For a remote database, replace `host.docker.internal` with the database host reachable from the containers.

Uploaded packages are mapped from the host into the server container:

```yaml
${MTY_PACKAGE_ROOT:-./data/packages}:/app/packages
```

Set it from the command line when needed:

```powershell
$env:MTY_PACKAGE_ROOT='D:\mty-packages'
docker compose up --build
```

Docker Compose reads variables from `.env` automatically. You can copy `.env.example` to `.env` and edit:

```powershell
Copy-Item .env.example .env
```

Important settings:

- `POSTGRES_CONNECTION_STRING`: external PostgreSQL connection string.
- `MTY_SERVER_PORT`: host port for API, defaults to `5000`.
- `MTY_WEB_PORT`: host port for web, defaults to `5173`.
- `MTY_PACKAGE_ROOT`: host directory for uploaded packages and `signing-key.json`.
- `MTY_JWT_ISSUER`, `MTY_JWT_AUDIENCE`, `MTY_JWT_SIGNING_KEY`: JWT settings.
- `MTY_ADMIN_USERNAME`, `MTY_ADMIN_PASSWORD`: initial admin seed.
- `VITE_API_BASE_URL`: API URL exposed to the web container at runtime.

The `MTY_*` and `VITE_API_BASE_URL` values are ordinary Docker Compose environment variables. You can set them in `.env`, set them in the shell before `docker compose up`, or inline for one command. PowerShell example:

```powershell
$env:MTY_SERVER_PORT='5080'
$env:MTY_WEB_PORT='5174'
$env:MTY_JWT_SIGNING_KEY='use-a-long-random-secret-at-least-32-chars'
docker compose up --build
```

Default development admin:

- username: `admin`
- password: `ChangeMe123!`

### CLI

```powershell
cd cli
cargo run -- init
cargo run -- system-info
cargo run -- --registry http://localhost:5000 search demo
cargo run -- --json list
cargo run -- outdated
cargo run -- update --dry-run
cargo run -- completion powershell
```

`mty --json` emits machine-readable results for commands; progress and errors go to stderr. `mty completion` prints a shell completion script for `bash`, `zsh`, `fish`, or `powershell`.

CLI text output uses semantic colors for headings, package names, versions, paths, progress, success, warnings, and errors. The global `--color auto|always|never` option defaults to `auto`, detecting stdout and stderr independently. Set `NO_COLOR` to disable automatic colors, use `mty --color always list` to force colors, or `mty --color never list` for plain text. JSON mode (including diagnostics) and completion scripts stay plain even with `--color always`.

The registry signs uploaded packages automatically. CLI install verifies signatures using `MTY_PUBLIC_KEY` when set, otherwise it fetches the registry public key from `/api/signing-key`.

By default MTY uses:

- Home: `%USERPROFILE%\.mty` on Windows, `$HOME/.mty` on Unix-like systems
- Installed files: `<MTY_HOME>\tools\<package>`
- Command shims: `<MTY_HOME>\bin`

`mty init` creates those folders and adds `<MTY_HOME>\bin` to the user PATH. Restart the terminal if newly installed commands are not found immediately.

### Web

```powershell
cd web
npm install
npm run dev
```

Configure `VITE_API_BASE_URL` when the API is not on `http://localhost:5000`. In containers this is read at runtime from the web service environment and written to `/env.js` when Caddy starts.
