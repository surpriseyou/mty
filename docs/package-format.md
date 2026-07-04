# `.mty` Package Format

An `.mty` package is a ZIP archive with a required `manifest.json` at the archive root.

```json
{
  "name": "demo-tool",
  "version": "1.0.0",
  "description": "Demo command line tool",
  "platform": "windows",
  "arch": "x64",
  "entry": "bin/demo.exe",
  "dependencies": [
    { "name": "runtime-tool", "version": ">=1.0.0" }
  ],
  "files": [
    {
      "path": "bin/demo.exe",
      "sha256": "hex-encoded-sha256",
      "executable": true
    }
  ]
}
```

Server metadata stores package-level SHA256 and an Ed25519 signature over the package hash. The server signs uploaded packages automatically.
Clients verify:

1. Download SHA256 equals server metadata.
2. Signature validates against a configured public key.
3. Every extracted file listed in `manifest.json` matches its SHA256.

The first version declares dependencies but does not install them automatically.

## Server-Generated Packages

The admin API can generate a `.mty` package from a single executable upload:

`POST /api/admin/packages/{name}/versions/from-executable`

Form fields:

- `version`: package version
- `platform`: `windows`, `linux`, or `macos`
- `arch`: `x86_64`, `x64`, `aarch64`, `arm64`, or `x86`
- `description`: optional package description
- `signature`: optional package signature over the generated package SHA256
- `file`: executable file

Generated archives contain:

- `manifest.json`
- `bin/<uploaded-file-name>`

The server signs generated packages automatically. Clients can use `GET /api/signing-key` to fetch the registry public key.

Self-update packages are ordinary packages named `mty`. The admin console provides a shortcut that uploads an executable as the `mty` package, and clients can query `GET /api/self-update?platform=windows&arch=x86_64` or download `GET /api/self-update/download?platform=windows&arch=x86_64`.
