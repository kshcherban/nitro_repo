# PHP Composer Repository Quick Reference

## Configuration Templates

### Hosted Repository
```json
{
  "type": "Hosted"
}
```

*Note: PHP repositories currently only support hosted mode. Proxy support is planned for future releases.*

## Essential Commands

### Configure Composer Repository
Add to your project's `composer.json`:
```json
{
  "name": "my/project",
  "type": "project",
  "require": {
    "mycompany/mypackage": "^1.0"
  },
  "repositories": [
    {
      "type": "composer",
      "url": "https://your-nitro-repo.example.com/repositories/storage/php-repo"
    }
  ]
}
```

### Configure Global Composer
Add to global `composer.json` (`~/.composer/composer.json`):
```json
{
  "repositories": [
    {
      "type": "composer",
      "url": "https://your-nitro-repo.example.com/repositories/storage/php-repo"
    }
  ]
}
```

### Configure Authentication
Add to `auth.json` (`~/.composer/auth.json` or project-level):
```json
{
  "http-basic": {
    "your-nitro-repo.example.com": {
      "username": "your-username",
      "password": "your-password"
    }
  }
}
```

### Install Package
```bash
# Install from configured repository
composer install

# Install specific package
composer require mycompany/mypackage

# Install with authentication
composer config --global http-basic.your-nitro-repo.example.com username password
composer install
```

### Update Packages
```bash
# Update all dependencies
composer update

# Update specific package
composer update mycompany/mypackage

# Update with verbose output
composer update -v
```

## Publishing Workflows

### Standard Composer Package
```bash
# Package structure
my-package/
├── composer.json
├── src/
│   ├── MyClass.php
│   └── helpers.php
├── tests/
│   └── MyClassTest.php
└── README.md
```

### composer.json Configuration
```json
{
  "name": "mycompany/mypackage",
  "description": "My awesome PHP package",
  "type": "library",
  "version": "1.0.0",
  "keywords": ["php", "composer", "package"],
  "license": "MIT",
  "authors": [
    {
      "name": "Your Name",
      "email": "your.email@example.com",
      "homepage": "https://yourwebsite.com"
    }
  ],
  "require": {
    "php": ">=7.4",
    "ext-json": "*"
  },
  "require-dev": {
    "phpunit/phpunit": "^9.0",
    "squizlabs/php_codesniffer": "^3.0"
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
    "check-style": "phpcs --standard=PSR12 src/",
    "fix-style": "phpcbf --standard=PSR12 src/"
  },
  "minimum-stability": "stable",
  "prefer-stable": true,
  "config": {
    "sort-packages": true
  },
  "archive": {
    "exclude": ["/tests", "/.git"]
  }
}
```

### Upload Package Archive
```bash
# Create package archive
composer archive --format=zip --dir=dist

# Upload to repository using curl
curl -X PUT \
     -u "username:password" \
     -T "dist/mycompany-mypackage-1.0.0.zip" \
     "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-1.0.0.zip"

# Update package metadata
curl -X PUT \
     -u "username:password" \
     -H "Content-Type: application/json" \
     -d @package.json \
     "https://your-nitro-repo.example.com/repositories/storage/php-repo/p/mycompany/mypackage.json"
```

### Update packages.json
Update the main repository index:
```bash
curl -X PUT \
     -u "username:password" \
     -H "Content-Type: application/json" \
     -d @packages.json \
     "https://your-nitro-repo.example.com/repositories/storage/php-repo/packages.json"
```

### Package Metadata (package.json)
```json
{
  "name": "mycompany/mypackage",
  "description": "My awesome PHP package",
  "version": "1.0.0",
  "type": "library",
  "keywords": ["php", "composer", "package"],
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
    "url": "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-1.0.0.zip",
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
    }
  },
  "time": "2023-12-15T10:00:00Z"
}
```

### Repository Index (packages.json)
```json
{
  "packages": {
    "mycompany/mypackage": {
      "1.0.0": {
        "name": "mycompany/mypackage",
        "description": "My awesome PHP package",
        "version": "1.0.0",
        "type": "library",
        "license": "MIT",
        "authors": [
          {
            "name": "Your Name",
            "email": "your.email@example.com"
          }
        ],
        "dist": {
          "type": "zip",
          "url": "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-1.0.0.zip",
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
      },
      "1.1.0": {
        "name": "mycompany/mypackage",
        "version": "1.1.0",
        "dist": {
          "type": "zip",
          "url": "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-1.1.0.zip",
          "reference": "v1.1.0",
          "shasum": "def456..."
        },
        "require": {
          "php": ">=7.4"
        },
        "autoload": {
          "psr-4": {
            "MyCompany\\MyPackage\\": "src/"
          }
        },
        "time": "2023-12-15T11:00:00Z"
      }
    }
  },
  "notify-batch": "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads",
  "search": "https://your-nitro-repo.example.com/repositories/storage/php-repo/search.json?q=%query%&type=%type%"
}
```

## Common Endpoints

| Description | Endpoint | Example |
|-------------|----------|---------|
| Repository Index | `GET /packages.json` | List all packages |
| Package Metadata | `GET /p/{package}.json` | Package information |
| Package Version | `GET /p/{package}/{version}.json` | Specific version info |
| Download Package | `GET /downloads/{package}-{version}.zip` | Download archive |
| Upload Package | `PUT /downloads/{package}-{version}.zip` | Upload new version |
| Search Packages | `GET /search.json` | Search packages |

## CI/CD Integration

### GitHub Actions Workflow
```yaml
name: Build and Publish Composer Package

on:
  push:
    tags: ['v*']

jobs:
  build-and-publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3

      - name: Setup PHP
        uses: shivammathur/setup-php@v2
        with:
          php-version: '8.1'
          extensions: json, curl
          coverage: none

      - name: Validate composer.json
        run: composer validate --strict

      - name: Cache Composer packages
        uses: actions/cache@v3
        with:
          path: vendor
          key: ${{ runner.os }}-php-${{ hashFiles('**/composer.lock') }}
          restore-keys: ${{ runner.os }}-php-

      - name: Install dependencies
        run: composer install --prefer-dist --no-progress

      - name: Run tests
        run: composer test

      - name: Create package archive
        run: |
          composer archive --format=zip --dir=dist \
            --version=${{ github.ref_name }}

      - name: Upload to Nitro Repo
        run: |
          # Upload package archive
          curl -X PUT \
               -u "${{ secrets.NITRO_USERNAME }}:${{ secrets.NITRO_PASSWORD }}" \
               -T "dist/mycompany-mypackage-${{ github.ref_name }}.zip" \
               "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-${{ github.ref_name }}.zip"

          # Update package metadata
          curl -X PUT \
               -u "${{ secrets.NITRO_USERNAME }}:${{ secrets.NITRO_PASSWORD }}" \
               -H "Content-Type: application/json" \
               -d @package.json \
               "https://your-nitro-repo.example.com/repositories/storage/php-repo/p/mycompany/mypackage.json"

          # Update repository index
          curl -X PUT \
               -u "${{ secrets.NITRO_USERNAME }}:${{ secrets.NITRO_PASSWORD }}" \
               -H "Content-Type: application/json" \
               -d @packages.json \
               "https://your-nitro-repo.example.com/repositories/storage/php-repo/packages.json"
```

### Automated Version Management
```bash
#!/bin/bash
# publish.sh - Automated publishing script

set -e

HOST="https://your-nitro-repo.example.com"
REPO_PATH="repositories/storage/php-repo"
PACKAGE_NAME="mycompany/mypackage"
VERSION=$1
USERNAME=$2
PASSWORD=$3

if [ -z "$VERSION" ] || [ -z "$USERNAME" ] || [ -z "$PASSWORD" ]; then
    echo "Usage: $0 <version> <username> <password>"
    exit 1
fi

echo "Publishing $PACKAGE_NAME version $VERSION"

# Create package archive
composer archive --format=zip --dir=dist --version=$VERSION

ARCHIVE_NAME="${PACKAGE_NAME/\//-}-$VERSION.zip"
ARCHIVE_PATH="dist/$ARCHIVE_NAME"

# Upload package archive
echo "Uploading package archive..."
curl -X PUT \
     -u "$USERNAME:$PASSWORD" \
     -T "$ARCHIVE_PATH" \
     "$HOST/$REPO_PATH/downloads/$ARCHIVE_NAME"

# Update package metadata
echo "Updating package metadata..."
cat > package.json << EOF
{
  "name": "$PACKAGE_NAME",
  "version": "$VERSION",
  "dist": {
    "type": "zip",
    "url": "$HOST/$REPO_PATH/downloads/$ARCHIVE_NAME",
    "reference": "$VERSION",
    "shasum": "$(sha1sum $ARCHIVE_PATH | cut -d' ' -f1)"
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

curl -X PUT \
     -u "$USERNAME:$PASSWORD" \
     -H "Content-Type: application/json" \
     -d @package.json \
     "$HOST/$REPO_PATH/p/mycompany/mypackage.json"

echo "Package $PACKAGE_NAME version $VERSION published successfully!"
```

## Testing Repository Access

### Test Composer Configuration
```bash
# Validate composer.json
composer validate

# Show repository configuration
composer config --list

# Show installed packages
composer show --installed

# Show package information
composer show mycompany/mypackage
```

### Test Repository Connectivity
```bash
# Test repository access
curl -u "username:password" \
     "https://your-nitro-repo.example.com/repositories/storage/php-repo/packages.json"

# Download package metadata
curl -u "username:password" \
     "https://your-nitro-repo.example.com/repositories/storage/php-repo/p/mycompany/mypackage.json"

# Test package download
curl -u "username:password" -O \
     "https://your-nitro-repo.example.com/repositories/storage/php-repo/downloads/mycompany-mypackage-1.0.0.zip"
```

### Debug Composer Issues
```bash
# Verbose install
composer install -v

# Very verbose with profiling
composer install -vvv --profile

# Clear composer cache
composer clear-cache

# Diagnose issues
composer diagnose
```

## Configuration Options

| Setting | Default | Description |
|---------|---------|-------------|
| Repository Type | Hosted | Currently only hosted mode supported |
| Authentication | Optional | Basic auth for write operations |
| Archive Format | ZIP | Standard package archive format |

## Package Types

### Library Package
```json
{
  "type": "library",
  "require": {
    "php": ">=7.4"
  }
}
```

### Project Package
```json
{
  "type": "project",
  "require": {
    "php": ">=8.0",
    "ext-pdo": "*"
  }
}
```

### Metapackage
```json
{
  "type": "metapackage",
  "require": {
    "mycompany/package1": "^1.0",
    "mycompany/package2": "^2.0"
  }
}
```

## Version Management

### Semantic Versioning
```bash
# Bump patch version (1.0.0 -> 1.0.1)
composer bump patch

# Bump minor version (1.0.0 -> 1.1.0)
composer bump minor

# Bump major version (1.0.0 -> 2.0.0)
composer bump major
```

### Development Versions
```json
{
  "version": "1.0.0-dev",
  "minimum-stability": "dev",
  "prefer-stable": false
}
```

## Troubleshooting Commands

### Common Issues
```bash
# Fix permission issues
sudo chown -R $USER:$(id -gn $USER) ~/.composer

# Clear composer cache
composer clear-cache

# Reinstall dependencies
composer install --no-dev --optimize-autoloader

# Update autoloader
composer dump-autoload
```

### Network Issues
```bash
# Disable SSL verification (development only)
composer config --global secure-http false

# Set proxy
composer config --global http-proxy http://proxy.example.com:8080

# Increase timeout
composer config --global process-timeout 600
```

## Security Checklist

- [ ] Use HTTPS for all repository communications
- [ ] Enable authentication for package uploads
- [ ] Validate package archives before publishing
- [ ] Use secure password storage for auth.json
- [ ] Monitor package access logs
- [ ] Implement code signing for packages
- [ ] Regular security audits of packages
- [ ] Use separate repositories for development and production

## Performance Tips

### For Large Packages
- Use `.gitattributes` to exclude unnecessary files from archives
- Optimize autoloader with `composer dump-autoload --optimize`
- Use classmap for better performance in production

### For Development
- Use `composer install --no-dev` for production deployments
- Enable composer cache for faster installs
- Use parallel downloads in Composer 2.x

---

*Quick reference for PHP/Composer repository configuration and usage. See [PHP Route Reference](php-route-reference.md) for detailed API documentation.*