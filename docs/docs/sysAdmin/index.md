# How to setup Nitro_Repo
## Pre Install Tasks
1. Install MySQL. For more information click [here](https://repo-docs.sudoers.dev/knowledge/InternalWorkings.html#users).
2. Create a database. For nitro_repo to use
## Getting your build
Please use one of the following options for your build
1. Latest [Release](https://github.com/kshcherban/nitro_repo/releases) on Github
2. Latest [Build](https://github.com/kshcherban/nitro_repo/actions/workflows/push.yml) on Github
3. Build yourself. Instructions are [here](https://repo-docs.sudoers.dev/compiling.html).  
   **Linux build prerequisites:** install `pkg-config` and the OpenSSL development headers (`libssl-dev` on Debian/Ubuntu, `openssl-devel` on Fedora/RHEL) before running `cargo build`.

## Setup
1. Decompress the build inside your install directory. I use `/opt/nitro_repo`. Using the command `tar -xf nitro_repo.tar.gz` Note: You might have to decompress the zip for Github Latest Builds
2. Run `./nitro_repo --install` Follow the CLI for installation. 
3. After completing the installation go ahead and run ./nitro_repo again. To ensure proper setup. Connect to it over the browser. Using your host and port set
4. Edit other/nitro_repo.service to use the appropriate location of your installation. Then copy the nitro_repo.service to the service directory Command: `cp other/nitro_repo.service /etc/systemd/system/nitro_repo.service`
5. Run `systemctl daemon-reload` and `systemctl start nitro_repo.service`
### SSL
After installation you can add SSL

Edit cfg/nitro_repo.toml

Under the application section

Add

```toml
ssl_private_key=
ssl_cert_key=
```

Make sure to specify values

#### For Lets Encrypt 

```toml
ssl_private_key='/etc/letsencrypt/live/{domain}/privkey.pem'
ssl_cert_key='/etc/letsencrypt/live/{domain}/cert.pem'
```
### 

Finally Restart Nitro Repo

## Enabling SSO Login
Nitro Repo can delegate authentication to an upstream SSO provider (Keycloak, Authelia, Dex, etc.) that injects identity headers after a successful login. Configure the security section in `cfg/nitro_repo.toml` to enable the feature:

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

- `login_path` is where the UI redirects users when clicking the "Sign in with SSO" button. Keep it pointed at Nitro Repo if your SSO proxy rewrites the request.
- `username_header`, `email_header`, and `display_name_header` must match the headers your proxy provides. Only `username_header` is required.
- Adjust `login_button_text` if you need a custom label on the login screen.
- `provider_login_url` can point to the IdP's login endpoint (for example, Cloudflare Access). Nitro Repo appends its own SSO callback URL using `provider_redirect_param` (defaults to `redirect`).
- When `auto_create_users` is `true`, Nitro Repo will create an account automatically on first login using the forwarded identity information. Set it to `false` to require manual user provisioning.

You can also manage these settings under **Admin → System → Single Sign-On** without editing configuration files or restarting the service.

Requests that reach `/api/user/sso/login` must already be authenticated by the upstream proxy; Nitro Repo only validates the forwarded headers, issues its own session cookie, and redirects back to the UI.
