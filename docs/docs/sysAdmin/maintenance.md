# Nitro Repository System Administrator Guide

This guide provides comprehensive documentation for system administrators deploying and managing Nitro Repository.

## Table of Contents

1. [Overview](#overview)
2. [Installation](#installation)
3. [Configuration](#configuration)
4. [Database Setup](#database-setup)
5. [OpenTelemetry and Monitoring](#opentelemetry-and-monitoring)
6. [Security Configuration](#security-configuration)
7. [SSL/TLS Setup](#ssltls-setup)
8. [Performance Tuning](#performance-tuning)
9. [Backup and Recovery](#backup-and-recovery)
10. [Troubleshooting](#troubleshooting)
11. [Maintenance](#maintenance)

## Overview

Nitro Repository is an open-source artifact management system designed for storing, versioning, and distributing software artifacts. It provides:

- **Artifact Storage**: Secure storage for packages, binaries, and other software artifacts
- **Version Management**: Semantic versioning and artifact lifecycle management
- **Access Control**: Fine-grained permissions and authentication
- **API Access**: RESTful API for integration with CI/CD pipelines
- **Monitoring**: Built-in observability with OpenTelemetry

## Installation

### Prerequisites

- **Operating System**: Linux (Ubuntu 20.04+, RHEL 8+, CentOS 8+)
- **Architecture**: x86_64 or ARM64
- **Memory**: Minimum 2GB RAM, recommended 4GB+
- **Storage**: Minimum 10GB for application, additional space for artifacts
- **Network**: Outbound HTTPS connectivity required

### Database Requirements

- **PostgreSQL**: Version 13 or later
- **Memory**: Minimum 1GB, recommended 2GB+
- **Storage**: SSD recommended, minimum 10GB

### Installation Steps

1. **Download the latest release**:
   ```bash
   wget https://github.com/your-org/nitro-repo/releases/latest/download/nitro-repo-linux-x86_64.tar.gz
   tar -xzf nitro-repo-linux-x86_64.tar.gz
   sudo cp nitro-repo /usr/local/bin/
   sudo chmod +x /usr/local/bin/nitro-repo
   ```

2. **Create system user**:
   ```bash
   sudo useradd --system --home /var/lib/nitro-repo --shell /bin/false nitro-repo
   sudo mkdir -p /var/lib/nitro-repo
   sudo chown nitro-repo:nitro-repo /var/lib/nitro-repo
   ```

3. **Create required directories**:
   ```bash
   sudo mkdir -p /etc/nitro-repo /var/log/nitro-repo /data/storages
   sudo chown nitro-repo:nitro-repo /data/storages
   ```

4. **Install configuration**:
   ```bash
   sudo cp examples/config.toml /etc/nitro-repo/config.toml
   sudo chown nitro-repo:nitro-repo /etc/nitro-repo/config.toml
   sudo chmod 600 /etc/nitro-repo/config.toml
   ```

### Systemd Service

Create a systemd service file:

```bash
sudo tee /etc/systemd/system/nitro-repo.service > /dev/null <<EOF
[Unit]
Description=Nitro Repository
After=network.target postgresql.service
Wants=postgresql.service

[Service]
Type=simple
User=nitro-repo
Group=nitro-repo
WorkingDirectory=/var/lib/nitro-repo
ExecStart=/usr/local/bin/nitro-repo --config /etc/nitro-repo/config.toml
Restart=always
RestartSec=5

# Security settings
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ReadWritePaths=/var/lib/nitro-repo /data/storages /var/log/nitro-repo

# Environment
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
EOF
```

Enable and start the service:

```bash
sudo systemctl daemon-reload
sudo systemctl enable nitro-repo
sudo systemctl start nitro-repo
sudo systemctl status nitro-repo
```

## Configuration

### Configuration File Location

Nitro Repository looks for configuration in the following order:

1. Path specified with `--config` flag
2. `/etc/nitro-repo/config.toml`
3. `./config.toml`
4. Environment variables passed to the process with prefix `NITRO`

### Core Configuration

```toml
# Application mode
mode = "Release"  # "Debug" or "Release"

# Local storage path for artifacts
suggested_local_storage_path = "/data/storages"
```

### Database Configuration

```toml
[database]
user = "nitro"
password = "secure-password"
database = "nitro_repo"
host = "localhost"
port = 5432
ssl_mode = "require"
max_connections = 10
```

### Web Server Configuration

```toml
[web_server]
bind_address = "0.0.0.0:6742"
open_api_routes = true

[web_server.max_upload]
max_size = "1GB"

# Optional TLS
[web_server.tls]
cert_path = "/etc/ssl/certs/nitro-repo.crt"
key_path = "/etc/ssl/private/nitro-repo.key"
```

### Environment Variables

All configuration options can be overridden via environment variables using the pattern `NITRO_{SECTION}__{KEY}`.

```bash
env \
  NITRO_DATABASE__HOST=localhost \
  NITRO_DATABASE__PASSWORD=secure-password \
  NITRO_WEB_SERVER__BIND_ADDRESS=0.0.0.0:6742 \
  nitro-repo --config /etc/nitro-repo/config.toml
```

## Database Setup

### PostgreSQL Installation

**Ubuntu/Debian:**
```bash
sudo apt update
sudo apt install postgresql postgresql-contrib
sudo systemctl start postgresql
sudo systemctl enable postgresql
```

**RHEL/CentOS:**
```bash
sudo dnf install postgresql-server postgresql-contrib
sudo postgresql-setup --initdb
sudo systemctl start postgresql
sudo systemctl enable postgresql
```

### Database Creation

```bash
# Switch to postgres user
sudo -u postgres psql

# Create database and user
CREATE DATABASE nitro_repo;
CREATE USER nitro WITH PASSWORD 'secure-password';
GRANT ALL PRIVILEGES ON DATABASE nitro_repo TO nitro;
ALTER USER nitro CREATEDB;
\q
```

### Database Migration

Nitro Repository automatically handles database migrations on startup. The first run will create all necessary tables and indexes.

### Performance Tuning

For production deployments, consider these PostgreSQL settings in `postgresql.conf`:

```ini
# Memory settings
shared_buffers = 256MB
effective_cache_size = 1GB
work_mem = 4MB
maintenance_work_mem = 64MB

# Connection settings
max_connections = 100

# WAL settings
wal_buffers = 16MB
checkpoint_completion_target = 0.9

# Autovacuum
autovacuum = on
autovacuum_max_workers = 3
```

## OpenTelemetry and Monitoring

### OpenTelemetry Configuration

Nitro Repository includes built-in OpenTelemetry support for distributed tracing and metrics:

```toml
[opentelemetry]
# Enable/disable tracing
enabled = true
protocol = "GRPC"
endpoint = "http://otel-collector:4317"
traces = true
logs = false

[opentelemetry.config]
"service.name" = "nitro-repo"
"service.version" = "3.0.0"
"service.environment" = "production"
```

### Environment Variable Control

OpenTelemetry can also be controlled via environment variables:

```bash
# Enable tracing
export NITRO_TRACING_ENABLED=1

# Configure collector endpoint
export OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317
```

**Note**: Configuration file settings take precedence over environment variables for OpenTelemetry.

### Monitoring Stack

A typical monitoring setup includes:

1. **OpenTelemetry Collector**: Collects traces and metrics
2. **Jaeger**: Trace visualization and analysis
3. **Prometheus**: Metrics collection and storage
4. **Grafana**: Dashboard and visualization

Example OpenTelemetry Collector configuration:

```yaml
# otel-collector-config.yaml
receivers:
  otlp:
    protocols:
      grpc:
        endpoint: 0.0.0.0:4317

processors:
  batch:

exporters:
  prometheus:
    endpoint: "0.0.0.0:8889"

  jaeger:
    endpoint: jaeger:14250
    tls:
      insecure: true

service:
  pipelines:
    traces:
      receivers: [otlp]
      processors: [batch]
      exporters: [jaeger]

    metrics:
      receivers: [otlp]
      processors: [batch]
      exporters: [prometheus]
```

## Security Configuration

### Authentication

**Basic Authentication (for development/testing):**
```toml
[security]
allow_basic_without_tokens = true
```

**Production authentication should use:**
- JWT tokens
- OAuth2/OpenID Connect
- SSO integration

### SSO Configuration

```toml
[security.sso]
enabled = true
login_path = "/api/user/sso/login"
login_button_text = "Sign in with SSO"
provider_login_url = "https://your-sso-provider.com/login"
provider_redirect_param = "redirect_url"
username_header = "X-Remote-User"
email_header = "X-Remote-Email"
display_name_header = "X-Remote-Name"
auto_create_users = true
```

### File Permissions

Ensure proper file permissions:

```bash
# Configuration files
sudo chmod 600 /etc/nitro-repo/config.toml
sudo chown nitro-repo:nitro-repo /etc/nitro-repo/config.toml

# Storage directories
sudo chmod 750 /data/storages
sudo chown nitro-repo:nitro-repo /data/storages

# Log files
sudo chmod 750 /var/log/nitro-repo
sudo chown nitro-repo:nitro-repo /var/log/nitro-repo
```

### Network Security

- Use firewall to restrict access to database and admin ports
- Implement reverse proxy with TLS termination
- Consider VPN or private network for admin access

## SSL/TLS Setup

### Self-Signed Certificate (Development)

```bash
# Generate private key
sudo openssl genrsa -out /etc/ssl/private/nitro-repo.key 2048

# Generate certificate
sudo openssl req -new -x509 -key /etc/ssl/private/nitro-repo.key \
  -out /etc/ssl/certs/nitro-repo.crt -days 365

# Set permissions
sudo chmod 600 /etc/ssl/private/nitro-repo.key
sudo chmod 644 /etc/ssl/certs/nitro-repo.crt
sudo chown nitro-repo:nitro-repo /etc/ssl/private/nitro-repo.key
```

### Let's Encrypt (Production)

```bash
# Install certbot
sudo apt install certbot

# Generate certificate
sudo certbot certonly --standalone -d repo.yourdomain.com

# Copy to Nitro Repository location
sudo cp /etc/letsencrypt/live/repo.yourdomain.com/fullchain.pem \
  /etc/ssl/certs/nitro-repo.crt
sudo cp /etc/letsencrypt/live/repo.yourdomain.com/privkey.pem \
  /etc/ssl/private/nitro-repo.key
sudo chown nitro-repo:nitro-repo /etc/ssl/private/nitro-repo.key
```

### Reverse Proxy Configuration

**Nginx example:**

```nginx
server {
    listen 80;
    server_name repo.yourdomain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name repo.yourdomain.com;

    ssl_certificate /etc/ssl/certs/nitro-repo.crt;
    ssl_certificate_key /etc/ssl/private/nitro-repo.key;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-RSA-AES256-GCM-SHA512:DHE-RSA-AES256-GCM-SHA512;
    ssl_prefer_server_ciphers off;

    location / {
        proxy_pass http://127.0.0.1:6742;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # For large uploads
        client_max_body_size 1G;
        proxy_read_timeout 300s;
        proxy_connect_timeout 75s;
    }
}
```

## Performance Tuning

### Storage Performance

- **Use SSD storage** for artifact storage and database
- **Configure RAID** for redundancy and performance
- **Monitor disk I/O** and storage usage
- **Implement cleanup policies** for old artifacts

### Application Performance

```toml
# Optimize session management
[sessions]
lifespan = 3600  # Shorter sessions for better security
cleanup_interval = 300  # More frequent cleanup

# Optimize staging
[staging]
max_size = "500MB"  # Limit staging size
cleanup_interval = 300  # Frequent cleanup
stale_timeout = 1800   # 30 minutes
```

### Database Performance

- **Regular vacuum and analyze**: Configure autovacuum appropriately
- **Monitor slow queries**: Use pg_stat_statements
- **Index optimization**: Add indexes for frequent queries
- **Connection pooling**: Consider PgBouncer for high-traffic deployments

### Resource Limits

```bash
# System limits for nitro-repo user
sudo tee /etc/security/limits.d/nitro-repo.conf > /dev/null <<EOF
nitro-repo soft nofile 65536
nitro-repo hard nofile 65536
nitro-repo soft nproc 4096
nitro-repo hard nproc 4096
EOF
```

## Backup and Recovery

### Database Backup

**Automated backup script:**

```bash
#!/bin/bash
# backup-db.sh

BACKUP_DIR="/backup/nitro-repo"
DATE=$(date +%Y%m%d_%H%M%S)
DB_NAME="nitro_repo"
DB_USER="nitro"

mkdir -p $BACKUP_DIR

# Create database backup
pg_dump -h localhost -U $DB_USER -d $DB_NAME | gzip > $BACKUP_DIR/db_$DATE.sql.gz

# Keep last 7 days of backups
find $BACKUP_DIR -name "db_*.sql.gz" -mtime +7 -delete

echo "Database backup completed: $BACKUP_DIR/db_$DATE.sql.gz"
```

**Set up cron job:**

```bash
# Edit crontab
sudo crontab -e

# Add daily backup at 2 AM
0 2 * * * /usr/local/bin/backup-db.sh
```

### Artifact Backup

```bash
#!/bin/bash
# backup-artifacts.sh

SOURCE_DIR="/data/storages"
BACKUP_DIR="/backup/nitro-repo/artifacts"
DATE=$(date +%Y%m%d_%H%M%S)

mkdir -p $BACKUP_DIR

# Incremental backup using rsync
rsync -av --delete $SOURCE_DIR/ $BACKUP_DIR/current/

# Create snapshot
cp -al $BACKUP_DIR/current $BACKUP_DIR/$DATE

# Keep last 30 days
find $BACKUP_DIR -maxdepth 1 -type d -name "[0-9]*" -mtime +30 -exec rm -rf {} \;
```

### Recovery Procedure

1. **Stop Nitro Repository:**
   ```bash
   sudo systemctl stop nitro-repo
   ```

2. **Restore database:**
   ```bash
   gunzip -c /backup/nitro-repo/db_YYYYMMDD_HHMMSS.sql.gz | psql -h localhost -U nitro -d nitro_repo
   ```

3. **Restore artifacts:**
   ```bash
   sudo rsync -av /backup/nitro-repo/artifacts/YYYYMMDD_HHMMSS/ /data/storages/
   sudo chown -R nitro-repo:nitro-repo /data/storages
   ```

4. **Start Nitro Repository:**
   ```bash
   sudo systemctl start nitro-repo
   ```

## Troubleshooting

### Common Issues

**Service won't start:**

```bash
# Check status
sudo systemctl status nitro-repo

# View logs
sudo journalctl -u nitro-repo -f

# Check configuration
sudo -u nitro-repo nitro-repo --config /etc/nitro-repo/config.toml --check
```

**Database connection issues:**

```bash
# Test database connection
psql -h localhost -U nitro -d nitro_repo

# Check PostgreSQL status
sudo systemctl status postgresql

# Check PostgreSQL logs
sudo tail -f /var/log/postgresql/postgresql-*.log
```

**Performance issues:**

```bash
# Check system resources
top
htop
iotop
df -h

# Check database performance
sudo -u postgres psql -d nitro_repo -c "SELECT * FROM pg_stat_activity;"

# Check slow queries
sudo -u postgres psql -d nitro_repo -c "SELECT query, calls, total_time, mean_time FROM pg_stat_statements ORDER BY total_time DESC LIMIT 10;"
```

### Log Analysis

**Application logs:**
```bash
# Real-time log monitoring
sudo tail -f /var/log/nitro-repo/nitro-repo.log

# Error filtering
grep -i error /var/log/nitro-repo/nitro-repo.log

# Performance analysis
grep "request completed" /var/log/nitro-repo/nitro-repo.log | tail -100
```

**System logs:**
```bash
# Systemd service logs
sudo journalctl -u nitro-repo --since "1 hour ago"

# System logs
sudo dmesg | tail

# Authentication logs
sudo tail -f /var/log/auth.log
```

### Health Checks

**API health endpoint:**
```bash
curl -f http://localhost:6742/api/health || echo "Service unhealthy"
```

**Database connectivity:**
```bash
sudo -u nitro-repo psql -h localhost -U nitro -d nitro_repo -c "SELECT 1;"
```

## Maintenance

### Regular Tasks

**Daily:**
- Monitor system logs and metrics
- Check available disk space
- Review backup status

**Weekly:**
- Review and rotate logs
- Update system packages
- Check for security updates

**Monthly:**
- Database maintenance (vacuum, analyze)
- Review and clean up old artifacts
- Performance tuning review
- Security audit

### Log Rotation

Configure log rotation in `/etc/logrotate.d/nitro-repo`:

```
/var/log/nitro-repo/*.log {
    daily
    missingok
    rotate 30
    compress
    delaycompress
    notifempty
    create 644 nitro-repo nitro-repo
    postrotate
        systemctl reload nitro-repo
    endscript
}
```

### System Updates

**Package updates:**
```bash
# Check for updates
sudo apt update && sudo apt list --upgradable

# Apply updates
sudo apt upgrade

# For RHEL/CentOS
sudo dnf check-update
sudo dnf upgrade
```

**Application updates:**
```bash
# Download new version
wget https://github.com/your-org/nitro-repo/releases/latest/download/nitro-repo-linux-x86_64.tar.gz

# Backup current version
sudo systemctl stop nitro-repo
sudo cp /usr/local/bin/nitro-repo /usr/local/bin/nitro-repo.backup

# Install new version
tar -xzf nitro-repo-linux-x86_64.tar.gz
sudo cp nitro-repo /usr/local/bin/
sudo chmod +x /usr/local/bin/nitro-repo

# Start service
sudo systemctl start nitro-repo
sudo systemctl status nitro-repo
```

## Support

For additional support:

- **Documentation**: Check the `/docs` directory in the repository
- **Issues**: Report bugs on GitHub Issues
- **Community**: Join our Discord/Slack community
- **Enterprise**: Contact enterprise@yourdomain.com for support contracts

## Version Information

This guide covers Nitro Repository version 3.0.0-BETA. Some features may change in future versions.
