# PHP Composer Repository HTTP Routes

Nitro Repo implements a Packagist-compatible API for PHP Composer repositories. This reference details every HTTP route, its purpose, and example usage. Replace placeholders with your own hostname, storage name, repository name, package name, and authentication credentials.

- `<host>` – Nitro Repo base URL (e.g. `nitro.example.com`)
- `<storage>` – Storage identifier that backs the repository
- `<repository>` – Repository name
- `<package>` – Composer package name (e.g. `mycompany/mypackage`)
- `<version>` – Package version (e.g. `1.0.0`)
- `<username>` – Username for basic authentication
- `<password>` – Password for basic authentication
- `<hash>` – SHA1 hash of package archive

## Repository Information

### Root Endpoint
Get repository information and metadata:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/"
```
**Response:**
```json
{
  "name": "php-repo",
  "description": "PHP Composer Repository",
  "type": "composer",
  "packages-count": 123,
  "versions-count": 456,
  "last-modified": "2023-12-15T10:00:00Z"
}
```

### Repository Index (packages.json)
Get the main repository index listing all packages:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/packages.json"
```
**Response:**
```json
{
  "packages": {
    "mycompany/core": {
      "1.0.0": {
        "name": "mycompany/core",
        "description": "Core library package",
        "version": "1.0.0",
        "type": "library",
        "license": "MIT",
        "authors": [
          {
            "name": "Author Name",
            "email": "author@example.com"
          }
        ],
        "source": {
          "type": "git",
          "url": "https://github.com/mycompany/core.git",
          "reference": "v1.0.0"
        },
        "dist": {
          "type": "zip",
          "url": "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-core-1.0.0.zip",
          "reference": "v1.0.0",
          "shasum": "abc123..."
        },
        "require": {
          "php": ">=7.4"
        },
        "autoload": {
          "psr-4": {
            "MyCompany\\Core\\": "src/"
          }
        },
        "time": "2023-12-15T10:00:00Z"
      },
      "1.1.0": {
        "name": "mycompany/core",
        "version": "1.1.0",
        "dist": {
          "type": "zip",
          "url": "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-core-1.1.0.zip",
          "reference": "v1.1.0",
          "shasum": "def456..."
        },
        "require": {
          "php": ">=7.4"
        },
        "autoload": {
          "psr-4": {
            "MyCompany\\Core\\": "src/"
          }
        },
        "time": "2023-12-15T11:00:00Z"
      }
    },
    "mycompany/utils": {
      "1.0.0": {
        "name": "mycompany/utils",
        "description": "Utility functions package",
        "version": "1.0.0",
        "dist": {
          "type": "zip",
          "url": "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-utils-1.0.0.zip",
          "reference": "v1.0.0",
          "shasum": "ghi789..."
        },
        "require": {
          "php": ">=7.4",
          "mycompany/core": "^1.0"
        },
        "autoload": {
          "psr-4": {
            "MyCompany\\Utils\\": "src/"
          }
        },
        "time": "2023-12-15T12:00:00Z"
      }
    }
  },
  "notify-batch": "https://<host>/repositories/<storage>/<repository>/downloads",
  "search": "https://<host>/repositories/<storage>/<repository>/search.json?q=%query%&type=%type%",
  "providers-url": "https://<host>/repositories/<storage>/<repository>/p/%package%.json",
  "providers-api": "https://<host>/repositories/<storage>/<repository>/provider.json"
}
```

## Package Operations

### Get Package Metadata
Retrieve metadata for a specific package including all versions:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/p/<package>.json"
```
**Example URL:**
```bash
curl -u "username:password" \
     "https://nitro.example.com/repositories/storage/php-repo/p/mycompany/mypackage.json"
```
**Response:**
```json
{
  "package": "mycompany/mypackage",
  "versions": {
    "1.0.0": {
      "name": "mycompany/mypackage",
      "description": "My awesome package",
      "version": "1.0.0",
      "type": "library",
      "keywords": ["php", "composer", "package"],
      "license": "MIT",
      "authors": [
        {
          "name": "Your Name",
          "email": "your.email@example.com",
          "homepage": "https://yourwebsite.com"
        }
      ],
      "source": {
        "type": "git",
        "url": "https://github.com/mycompany/mypackage.git",
        "reference": "v1.0.0"
      },
      "dist": {
        "type": "zip",
        "url": "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-mypackage-1.0.0.zip",
        "reference": "v1.0.0",
        "shasum": "abc123..."
      },
      "require": {
        "php": ">=7.4",
        "ext-json": "*"
      },
      "require-dev": {
        "phpunit/phpunit": "^9.0"
      },
      "autoload": {
        "psr-4": {
          "MyCompany\\MyPackage\\": "src/"
        },
        "files": ["src/helpers.php"]
      },
      "autoload-dev": {
        "psr-4": {
          "MyCompany\\MyPackage\\Tests\\": "tests/"
        }
      },
      "scripts": {
        "test": "phpunit",
        "check-style": "phpcs --standard=PSR12 src/"
      },
      "minimum-stability": "stable",
      "prefer-stable": true,
      "config": {
        "sort-packages": true
      },
      "time": "2023-12-15T10:00:00Z"
    }
  }
}
```

### Get Specific Version
Retrieve metadata for a specific package version:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/p/<package>/<version>.json"
```
**Example URL:**
```bash
curl -u "username:password" \
     "https://nitro.example.com/repositories/storage/php-repo/p/mycompany/mypackage/1.0.0.json"
```
**Response:**
```json
{
  "name": "mycompany/mypackage",
  "description": "My awesome package",
  "version": "1.0.0",
  "type": "library",
  "license": "MIT",
  "authors": [
    {
      "name": "Your Name",
      "email": "your.email@example.com"
    }
  ],
  "source": {
    "type": "git",
    "url": "https://github.com/mycompany/mypackage.git",
    "reference": "v1.0.0"
  },
  "dist": {
    "type": "zip",
    "url": "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-mypackage-1.0.0.zip",
    "reference": "v1.0.0",
    "shasum": "abc123..."
  },
  "require": {
    "php": ">=7.4"
  },
  "autoload": {
    "psr-4": {
      "MyCompany\\MyPackage\\": "src/"
    }
  },
  "time": "2023-12-15T10:00:00Z"
}
```

## Package Archive Operations

### Download Package Archive
Download the package zip archive:
```bash
curl -u "<username>:<password>" -O \
     "https://<host>/repositories/<storage>/<repository>/downloads/<package>-<version>.zip"
```
**Example:**
```bash
curl -u "username:password" -O \
     "https://nitro.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-1.0.0.zip"
```

### Upload Package Archive
Upload a new package version archive:
```bash
curl -X PUT \
     -u "<username>:<password>" \
     -T "mycompany-mypackage-1.0.0.zip" \
     "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-mypackage-1.0.0.zip"
```

### Check Archive Exists
Check if a package archive exists without downloading:
```bash
curl -I -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-mypackage-1.0.0.zip"
```

### Delete Package Archive
Remove a package archive:
```bash
curl -X DELETE \
     -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-mypackage-1.0.0.zip"
```

## Package Metadata Operations

### Update Package Metadata
Update or create package metadata:
```bash
curl -X PUT \
     -u "<username>:<password>" \
     -H "Content-Type: application/json" \
     -d @package.json \
     "https://<host>/repositories/<storage>/<repository>/p/<package>.json"
```

**Request Body (package.json):**
```json
{
  "name": "mycompany/mypackage",
  "description": "My awesome package",
  "version": "1.0.0",
  "type": "library",
  "license": "MIT",
  "authors": [
    {
      "name": "Your Name",
      "email": "your.email@example.com"
    }
  ],
  "source": {
    "type": "git",
    "url": "https://github.com/mycompany/mypackage.git",
    "reference": "v1.0.0"
  },
  "dist": {
    "type": "zip",
    "url": "https://<host>/repositories/<storage>/<repository>/downloads/mycompany-mypackage-1.0.0.zip",
    "reference": "v1.0.0",
    "shasum": "abc123..."
  },
  "require": {
    "php": ">=7.4"
  },
  "autoload": {
    "psr-4": {
      "MyCompany\\MyPackage\\": "src/"
    }
  },
  "time": "2023-12-15T10:00:00Z"
}
```

### Update Specific Version
Update metadata for a specific version:
```bash
curl -X PUT \
     -u "<username>:<password>" \
     -H "Content-Type: application/json" \
     -d @version.json \
     "https://<host>/repositories/<storage>/<repository>/p/<package>/<version>.json"
```

## Repository Index Operations

### Update Repository Index
Update the main packages.json index:
```bash
curl -X PUT \
     -u "<username>:<password>" \
     -H "Content-Type: application/json" \
     -d @packages.json \
     "https://<host>/repositories/<storage>/<repository>/packages.json"
```

### Get Provider Information
Get provider information for efficient package resolution:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/provider.json"
```
**Response:**
```json
{
  "providers": {
    "mycompany/mypackage": {
      "sha256": "def456..."
    }
  },
  "providers-includes": {
    "p/provider-latest.json": {
      "sha256": "abc123..."
    }
  }
}
```

## Search Operations

### Search Packages
Search packages by name, description, or keywords:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/search.json?q=my-search-term&type=library&per_page=20&page=1"
```
**Response:**
```json
{
  "results": [
    {
      "name": "mycompany/mypackage",
      "description": "My awesome package",
      "url": "https://<host>/repositories/<storage>/<repository>/p/mycompany/mypackage.json",
      "repository": "https://<host>/repositories/<storage>/<repository>/",
      "downloads": 1234,
      "favers": 56
    }
  ],
  "total": 1,
  "next": "https://<host>/repositories/<storage>/<repository>/search.json?q=my-search-term&page=2"
}
```

### List All Packages
Get a list of all package names:
```bash
curl -u "<username>:<password>" \
     "https://<host>/repositories/<storage>/<repository>/packages/list.json"
```
**Response:**
```json
{
  "packageNames": [
    "mycompany/core",
    "mycompany/mypackage",
    "mycompany/utils",
    "vendor/library"
  ]
}
```

## Complete Publishing Workflow

### Full Package Publishing Sequence
```bash
#!/bin/bash
HOST="https://nitro.example.com"
STORAGE="storage"
REPO="php-repo"
PACKAGE="mycompany/mypackage"
VERSION="1.0.0"
USER="username"
PASS="password"

# Calculate file hash
ARCHIVE_NAME="${PACKAGE/\//-}-${VERSION}.zip"
HASH=$(sha1sum "$ARCHIVE_NAME" | cut -d' ' -f1)

# 1. Upload package archive
echo "Uploading package archive..."
curl -X PUT \
     -u "${USER}:${PASS}" \
     -T "$ARCHIVE_NAME" \
     "${HOST}/repositories/${STORAGE}/${REPO}/downloads/${ARCHIVE_NAME}"

# 2. Create package metadata
cat > package.json << EOF
{
  "name": "${PACKAGE}",
  "description": "My awesome package",
  "version": "${VERSION}",
  "type": "library",
  "license": "MIT",
  "authors": [
    {
      "name": "Your Name",
      "email": "your.email@example.com"
    }
  ],
  "source": {
    "type": "git",
    "url": "https://github.com/mycompany/mypackage.git",
    "reference": "v${VERSION}"
  },
  "dist": {
    "type": "zip",
    "url": "${HOST}/repositories/${STORAGE}/${REPO}/downloads/${ARCHIVE_NAME}",
    "reference": "v${VERSION}",
    "shasum": "${HASH}"
  },
  "require": {
    "php": ">=7.4"
  },
  "autoload": {
    "psr-4": {
      "MyCompany\\\\MyPackage\\\\": "src/"
    }
  },
  "time": "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
}
EOF

# 3. Upload package metadata
echo "Updating package metadata..."
curl -X PUT \
     -u "${USER}:${PASS}" \
     -H "Content-Type: application/json" \
     -d @package.json \
     "${HOST}/repositories/${STORAGE}/${REPO}/p/${PACKAGE}.json"

# 4. Get current repository index
echo "Updating repository index..."
curl -u "${USER}:${PASS}" \
     "${HOST}/repositories/${STORAGE}/${REPO}/packages.json" \
     > packages.json

# 5. Update packages.json with new package (would require script to properly merge)
# This step would typically be automated by the repository system

echo "Package ${PACKAGE} version ${VERSION} published successfully!"
```

## Repository Management API

### List Cached Packages
List cached PHP packages:
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/api/repository/<repository-id>/packages?page=1&per_page=50"
```

### Delete Cached Packages
Remove cached packages (requires repository edit permission):
```bash
curl -X DELETE \
     -H "Authorization: Bearer <token>" \
     -H "Content-Type: application/json" \
     -d '{"paths": ["downloads/mycompany-mypackage-1.0.0.zip", "p/mycompany/mypackage.json"]}' \
     "https://<host>/api/repository/<repository-id>/packages"
```

### Repository Configuration
Get PHP repository configuration:
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/api/repository/<repository-id>/config/php"
```

Update repository configuration:
```bash
curl -X PUT \
     -H "Authorization: Bearer <token>" \
     -H "Content-Type: application/json" \
     -d @config.json \
     "https://<host>/api/repository/<repository-id>/config/php"
```

### Repository Statistics
Get repository usage statistics:
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/api/repository/<repository-id>/stats"
```
**Response:**
```json
{
  "total_packages": 67,
  "total_versions": 234,
  "total_downloads": 5678,
  "storage_used": "890MB",
  "last_activity": "2023-12-15T15:30:00Z",
  "top_packages": [
    {"name": "mycompany/core", "download_count": 1234},
    {"name": "mycompany/utils", "download_count": 987}
  ]
}
```

## Composer Client Integration

### Typical Composer Workflow
When Composer needs to resolve dependencies:

1. **GET /packages.json** - Get repository index
2. **GET /p/{package}.json** - Get package metadata
3. **GET /downloads/{package}-{version}.zip** - Download package archive
4. **Extract and install** - Install package in vendor directory

### Composer Configuration
```json
{
  "repositories": [
    {
      "type": "composer",
      "url": "https://nitro.example.com/repositories/storage/php-repo"
    }
  ],
  "require": {
    "mycompany/mypackage": "^1.0"
  }
}
```

## Authentication

### Basic Authentication
All write operations require authentication:
```bash
curl -u "username:password" \
     "https://<host>/repositories/<storage>/<repository>/packages.json"
```

### API Token Authentication
Management endpoints use Bearer tokens:
```bash
curl -H "Authorization: Bearer <token>" \
     "https://<host>/api/repository/<repository-id>/stats"
```

## Error Responses

### Common Error Codes
- `401 Unauthorized` - Authentication required or invalid credentials
- `403 Forbidden` - Insufficient permissions
- `404 Not Found` - Package, version, or archive does not exist
- `409 Conflict` - Package version already exists
- `422 Unprocessable Entity` - Invalid package metadata
- `500 Internal Server Error` - Server-side error during upload

### Error Response Format
```json
{
  "error": "Not Found",
  "message": "Package mycompany/nonexistent not found",
  "details": {
    "package": "mycompany/nonexistent",
    "repository": "php-repo"
  }
}
```

## Storage Layout

```
repositories/
└── storage/
    └── php-repo/
        ├── packages.json
        ├── provider.json
        ├── p/
        │   ├── mycompany/
        │   │   ├── core.json
        │   │   ├── mypackage.json
        │   │   └── utils.json
        │   └── vendor/
        │       └── library.json
        └── downloads/
            ├── mycompany-core-1.0.0.zip
            ├── mycompany-mypackage-1.0.0.zip
            └── vendor-library-1.2.3.zip
```

Use these endpoints as a foundation for Composer client integration, build tool configuration, or custom tooling when working with Nitro Repo PHP repositories.

---

*Complete reference for PHP/Composer repository HTTP routes. See [PHP Quick Reference](php-quick-reference.md) for usage examples and configuration.*