# Single Sign-On (SSO)

Nitro Repo can sit behind an external identity provider (Keycloak, Authelia, Dex, OAuth2 Proxy, etc.) and accept pre-authenticated requests. When enabled, Nitro Repo trusts HTTP headers injected by this upstream SSO layer and issues its own session cookie for subsequent API calls.

## How It Works

1. Your reverse proxy directs the user to the identity provider.
2. After a successful login, the proxy forwards the request to Nitro Repo and injects identity headers (for example `X-Forwarded-User`).
3. Nitro Repo reads these headers at `/api/user/sso/login`, optionally creates the user, and starts a Nitro Repo session.

## Configuration

Configure the optional `security.sso` block inside `cfg/nitro_repo.toml` (or the corresponding environment variables):

```toml
[security.sso]
enabled = true
login_path = "/api/user/sso/login"
login_button_text = "Sign in with SSO"
provider_login_url = "https://example.com/login"
provider_redirect_param = "redirect"
username_header = "X-Forwarded-User"
email_header = "X-Forwarded-Email"
display_name_header = "X-Forwarded-Name"
auto_create_users = true
```

| Field | Description |
|-------|-------------|
| `enabled` | Turns the feature on. Disabled configs are ignored. |
| `login_path` | URL the frontend redirects to when the user clicks the SSO button. Usually keep it pointed at Nitro Repo so the proxy handles the request. |
| `login_button_text` | Customize the label displayed on the login screen. |
| `provider_login_url` | Optional IdP entrypoint. When set, the frontend hits this URL first and passes the Nitro Repo SSO endpoint as a redirect. |
| `provider_redirect_param` | Query parameter used by the provider login URL for the return location (defaults to `redirect`). |
| `username_header` | **Required.** Header containing a unique identifier for the user (e.g., `X-Forwarded-User`). |
| `email_header` | Optional email address forwarded from the IdP. Used when creating new accounts. |
| `display_name_header` | Optional display name shown in Nitro Repo. Falls back to the username when omitted. |
| `auto_create_users` | When `true`, Nitro Repo creates user records automatically the first time someone logs in via SSO. Set to `false` if you want admins to pre-provision accounts. |

### Runtime Configuration

Administrators can update these settings without restarting the server under **Admin → System → Single Sign-On**. Changes persist in the database and are visible immediately on the login screen.

## Frontend Experience

When SSO is enabled, the login page shows a "Sign in with SSO" button above the traditional username/password form. Users bypass the password flow entirely once the SSO proxy authenticates them. The frontend app automatically handles redirect targets (e.g., deep links to repository pages) when returning from `/api/user/sso/login`.

## Reverse Proxy Checklist

- Ensure the upstream SSO proxy terminates TLS and authenticates users **before** forwarding to Nitro Repo.
- Strip or overwrite the identity headers so untrusted clients cannot spoof them.
- Forward the necessary headers (`username_header`, etc.) on authenticated requests only.
- Allow access to `/api/user/sso/login`, `/api/user/logout`, and the SPA assets.

With these pieces in place, Nitro Repo delegates authentication to your enterprise IdP while retaining its existing session and authorization model.

## Example: Cloudflare One (One-Time PIN)

When Nitro Repo runs behind a Cloudflare Access application, Cloudflare authenticates users and adds identity headers to the proxied request. For the One-Time PIN IdP:

- `CF-Access-Authenticated-User-Email` contains the user’s verified email address.
- `CF-Access-Authenticated-User-Name` (if present) contains a friendly display name.
- Every authenticated request also carries a signed `Cf-Access-Jwt-Assertion` header that Cloudflare validates before forwarding to your origin.

Configure Nitro Repo to trust those headers by updating `docker/config/nitro_repo.toml`:

```toml
[security]
allow_basic_without_tokens = false

  [security.sso]
  enabled = true
  login_path = "/api/user/sso/login"
  login_button_text = "Sign in with Cloudflare"
  provider_login_url = "https://tv.sudoers.dev/cdn-cgi/access/login"
  provider_redirect_param = "redirect_url"
  username_header = "CF-Access-Authenticated-User-Email"
  email_header = "CF-Access-Authenticated-User-Email"
  display_name_header = "CF-Access-Authenticated-User-Name"
  auto_create_users = true
```

Cloudflare automatically blocks unauthenticated traffic, so Nitro Repo only receives requests that already satisfy your Access policies. If the identity headers are unavailable, Nitro Repo will fall back to decoding the `Cf-Access-Jwt-Assertion` token that Cloudflare always sends, so you do not need a Worker to inject extra headers. If you later add group-based policies, you can surface them by reading additional headers like `CF-Access-Roles`.

## Example: Google Workspace via OAuth2 Proxy

When fronting Nitro Repo with an OAuth2 reverse proxy (such as [oauth2-proxy](https://oauth2-proxy.github.io/oauth2-proxy/)) that uses Google Workspace as the identity provider, configure the proxy to include headers containing the user’s email and name:

```yaml
# oauth2-proxy excerpt
upstreams:
  - https://nitro_repo:6742

providers:
  - id: google-workspace
    provider: google
    clientID: ${GOOGLE_CLIENT_ID}
    clientSecret: ${GOOGLE_CLIENT_SECRET}
    scope: "openid email profile"
    google:
      adminEmail: admin@example.com
      group: nitro-users@example.com

setAuthorizationHeader: true
passAccessToken: true
passAuthorization: true
passUserHeaders: true
setXAuthRequest: true
```

Then point Nitro Repo at the headers added by oauth2-proxy:

```toml
[security.sso]
enabled = true
login_path = "/api/user/sso/login"
login_button_text = "Sign in with Google"
username_header = "X-Auth-Request-Email"
email_header = "X-Auth-Request-Email"
display_name_header = "X-Auth-Request-User"
auto_create_users = true
```

In the Nitro UI, set the provider login URL to the oauth2-proxy entrypoint (for example `https://login.example.com/oauth2/start`) and the redirect parameter to the name oauth2-proxy expects (`rd` by default). Nitro Repo will redirect users to oauth2-proxy, which authenticates with Google Workspace, injects the configured headers, and the `/api/user/sso/login` endpoint will complete the session.

### Docker Compose Example

Below is a minimal docker-compose setup that runs Nitro Repo behind oauth2-proxy and routes traffic through a single Nginx reverse proxy. OAuth2 proxy handles Google Workspace authentication and forwards identity headers to Nitro Repo.

```yaml
services:
  nitro_repo:
    image: nitro_repo-nitro_repo:latest
    env_file: .env
    volumes:
      - nitro_data:/data
    networks: [nitro_net]

  oauth2_proxy:
    image: quay.io/oauth2-proxy/oauth2-proxy:v7.7.1
    command:
      - --http-address=0.0.0.0:4180
      - --upstream=http://nitro_repo:6742
      - --cookie-secret=${OAUTH2_PROXY_COOKIE_SECRET}
      - --cookie-secure=true
      - --cookie-domain=example.com
      - --cookie-refresh=24h
      - --cookie-expire=168h
      - --reverse-proxy=true
      - --pass-access-token=true
      - --pass-authorization-header=true
      - --set-authorization-header=true
      - --set-xauthrequest=true
      - --skip-provider-button=true
      - --provider=google
      - --client-id=${GOOGLE_CLIENT_ID}
      - --client-secret=${GOOGLE_CLIENT_SECRET}
      - --redirect-url=https://login.example.com/oauth2/callback
      - --email-domain=example.com
      - --scope=openid email profile
      - --oidc-extra-audience=https://www.googleapis.com/auth/userinfo.email
    environment:
      OAUTH2_PROXY_COOKIE_SECRET: ${OAUTH2_PROXY_COOKIE_SECRET}
    networks: [nitro_net]

  nginx:
    image: nginx:stable
    depends_on:
      - oauth2_proxy
    volumes:
      - ./nginx.conf:/etc/nginx/conf.d/default.conf:ro
    ports:
      - 80:80
      - 443:443
    networks: [nitro_net]

networks:
  nitro_net:
    driver: bridge

volumes:
  nitro_data:
```

Example `nginx.conf` forwarding public traffic through oauth2-proxy:

```nginx
server {
  listen 80;
  server_name login.example.com;

  location / {
    proxy_pass http://oauth2_proxy:4180;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
  }
}

server {
  listen 80;
  server_name tv.sudoers.dev;

  location / {
    proxy_pass http://oauth2_proxy:4180;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
  }
}
```

Appoint Nitro Repo’s SSO settings to this proxy:

```toml
[security.sso]
enabled = true
login_path = "/api/user/sso/login"
login_button_text = "Sign in with Google"
provider_login_url = "https://login.example.com/oauth2/start"
provider_redirect_param = "rd"
username_header = "X-Auth-Request-Email"
email_header = "X-Auth-Request-Email"
display_name_header = "X-Auth-Request-User"
auto_create_users = true
```

With this setup, users hit `https://tv.sudoers.dev`, oauth2-proxy enforces Google Workspace login, injects the user headers, and Nitro Repo completes the SSO flow with its own session cookie.
