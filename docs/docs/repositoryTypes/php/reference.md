# PHP Composer Repository Quick Reference (V2)

## Configuration

### Repository config (hosted)
```json
{ "type": "Hosted" }
```
*Proxy coming soon.*

### Composer client configuration
`composer.json` (project):
```json
{
  "repositories": [
    { "type": "composer", "url": "https://your-host/repositories/<storage>/<php-repo>" }
  ],
  "require": { "mycompany/mypackage": "^1.0" }
}
```

`auth.json`:
```json
{
  "http-basic": {
    "your-host": { "username": "user", "password": "pass" }
  }
}
```

## Upload (hosted)
- Zip must contain `composer.json` with valid `name` (`vendor/package`) and `version`.
- Upload to the dist path (streamed, no packages.json edits needed):
```bash
curl -u user:pass -X PUT \
  -T dist/mycompany-mypackage-1.0.0.zip \
  https://your-host/repositories/<storage>/<php-repo>/dist/mycompany/mypackage/1.0.0.zip
```
Nitro Repo extracts `composer.json`, validates name/version against the path, rewrites `dist.url`
back to Nitro, and writes p2 metadata to `/p2/mycompany/mypackage.json` (plus `~dev` when relevant).

## Client fetch paths (Composer V2)
- Root: `/repositories/<storage>/<php-repo>/packages.json` (contains only `metadata-url`)
- Metadata: `/repositories/<storage>/<php-repo>/p2/<vendor>/<package>.json`
- Dist: `/repositories/<storage>/<php-repo>/dist/<vendor>/<package>/<version>.zip`

## Useful Composer commands
```bash
composer install                # install all deps
composer require mycompany/mypackage:^1.0
composer update mycompany/mypackage
composer config --global http-basic.your-host user pass
```

## Package layout reminder
```
my-package/
├── composer.json
├── src/
└── tests/
```

Minimal `composer.json`:
```json
{
  "name": "mycompany/mypackage",
  "version": "1.0.0",
  "type": "library",
  "require": { "php": ">=7.4" },
  "autoload": { "psr-4": { "MyCompany\\MyPackage\\": "src/" } }
}
```
