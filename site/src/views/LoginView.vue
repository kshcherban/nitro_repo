<template>
  <main>
    <h1>Login Page</h1>
    <section
      v-if="primaryFederated"
      class="ssoLogin">
      <button
        type="button"
        class="ssoButton"
        @click="federatedHandler(primaryFederated)">
        {{ primaryFederated.label }}
      </button>
      <p
        class="ssoHelp"
        v-if="showAutoProvisionMessage">
        Your account will be created automatically on first login.
      </p>
    </section>
    <section
      v-if="secondaryProviders.length > 0"
      class="oauthLogin">
      <button
        v-for="provider in secondaryProviders"
        :key="provider.provider"
        type="button"
        class="oauthButton"
        @click="startOAuth(provider.provider)">
        Sign in with {{ providerLabel(provider.provider) }}
      </button>
    </section>
    <div
      v-if="hasFederatedLogin"
      class="separator">
      <span>or</span>
    </div>
    <form @submit.prevent="login">
      <h4 v-if="failedLogin">Invalid username or password</h4>
      <TextInput
        id="username"
        v-model="input.email_or_username"
        autocomplete="username"
        autocapitalize="false"
        required
        autofocus
        placeholder="Username or Email">
        Username or Email
      </TextInput>
      <PasswordInput
        id="password"
        v-model="input.password"
        required
        >Password</PasswordInput
      >
      <div class="forgotPassword">
        <router-link to="/forgot-password">Forgot Password?</router-link>
      </div>
      <SubmitButton title="Login">Login</SubmitButton>
    </form>
  </main>
</template>
<script setup lang="ts">
import SubmitButton from "@/components/form/SubmitButton.vue";
import PasswordInput from "@/components/form/text/PasswordInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import http from "@/http";
import router from "@/router";
import { sessionStore } from "@/stores/session";
import { siteStore } from "@/stores/site";
import { notify } from "@kyvg/vue3-notification";
import type { InstanceOAuth2Provider } from "@/types/base";
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
const failedLogin = ref(false);
const input = ref({
  email_or_username: "",
  password: "",
});
const session = sessionStore();
const site = siteStore();
const route = useRoute();
const redirectTarget = computed(() => {
  const redirect = route.query.redirect;
  if (typeof redirect === "string" && redirect.startsWith("/") && !redirect.startsWith("//")) {
    return redirect;
  }
  return "/";
});
const oauthProviders = computed<InstanceOAuth2Provider[]>(() => {
  const providers = site.siteInfo?.oauth2?.providers;
  return Array.isArray(providers) ? providers : [];
});
const ssoEnabled = computed(() => Boolean(site.siteInfo?.sso));
const hasFederatedLogin = computed(
  () => ssoEnabled.value || oauthProviders.value.length > 0,
);
const ssoButtonText = computed(
  () => site.siteInfo?.sso?.login_button_text ?? "Sign in with SSO",
);
type FederatedTarget =
  | { kind: "sso"; label: string }
  | { kind: "oauth"; label: string; provider: InstanceOAuth2Provider };

const primaryFederated = computed<FederatedTarget | null>(() => {
  if (ssoEnabled.value) {
    return { kind: "sso" as const, label: ssoButtonText.value };
  }
  return oauthProviders.value[0]
    ? {
        kind: "oauth" as const,
        label: `Sign in with ${providerLabel(oauthProviders.value[0]!.provider)}`,
        provider: oauthProviders.value[0]!,
      }
    : null;
});
const secondaryProviders = computed<InstanceOAuth2Provider[]>(() => {
  if (ssoEnabled.value) {
    return oauthProviders.value;
  }
  return oauthProviders.value.slice(1);
});
const showAutoProvisionMessage = computed(() => {
  if (ssoEnabled.value) {
    return site.siteInfo?.sso?.auto_create_users ?? false;
  }
  return site.siteInfo?.oauth2?.auto_create_users ?? false;
});
async function login() {
  http
    .post("/api/user/login", input.value)
    .then((response) => {
      console.log(response);
      session.login(response.data);
      router.push(redirectTarget.value);
    })
    .catch((error) => {
      if (error.response.status === 401) {
        failedLogin.value = true;
        notify({
          type: "error",
          title: "Login Failed",
          text: "Invalid username or password",
        });
      } else {
        console.log(error);
        notify({
          type: "error",
          title: "Login Failed",
          text: "An error occurred while trying to login",
        });
      }
    });
}
function startSso() {
  const loginPath = site.siteInfo?.sso?.login_path ?? "/api/user/sso/login";
  try {
    const ssoUrl = resolveUrl(loginPath);
    ssoUrl.searchParams.set("redirect", redirectTarget.value);

    const providerUrl = site.siteInfo?.sso?.provider_login_url ?? undefined;
    if (providerUrl && providerUrl !== "") {
      const providerTarget = resolveUrl(providerUrl);
      const redirectParam =
        site.siteInfo?.sso?.provider_redirect_param?.trim() ?? "redirect";
      providerTarget.searchParams.set(redirectParam, ssoUrl.toString());
      window.location.href = providerTarget.toString();
    } else {
      window.location.href = ssoUrl.toString();
    }
  } catch (error) {
    console.error("Invalid SSO configuration", error);
  }
}

function resolveUrl(target: string): URL {
  if (target.startsWith("http://") || target.startsWith("https://")) {
    return new URL(target);
  }
  const normalized = target.startsWith("/") ? target : `/${target}`;
  return new URL(normalized, window.location.origin);
}

function sanitizeBase(path: string): string {
  return path.endsWith("/") ? path.slice(0, -1) : path;
}

function startOAuth(provider: string) {
  const basePath = site.siteInfo?.oauth2?.login_path ?? "/api/user/oauth2/login";
  const targetPath = `${sanitizeBase(basePath)}/${provider}`;
  const oauthUrl = resolveUrl(targetPath);
  oauthUrl.searchParams.set("redirect", redirectTarget.value);
  window.location.href = oauthUrl.toString();
}

function providerLabel(provider: string): string {
  switch (provider.toLowerCase()) {
    case "google":
      return "Google";
    case "microsoft":
      return "Microsoft";
    default:
      return provider.charAt(0).toUpperCase() + provider.slice(1);
  }
}

function federatedHandler(target: FederatedTarget) {
  if (target.kind === "sso") {
    startSso();
  } else {
    startOAuth(target.provider.provider);
  }
}

onMounted(async () => {
  await site.getInfo();
});
</script>
<style scoped lang="scss">
@import "@/assets/styles/theme.scss";
main {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 100vh;
}
form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.ssoLogin {
  display: flex;
  flex-direction: column;
  align-items: center;
  margin-bottom: 1.5rem;
}
.ssoButton {
  background-color: $primary-70;
  color: $background;
  padding: 0.75rem 1.5rem;
  border: none;
  border-radius: 0.5rem;
  font-size: 1.1rem;
  cursor: pointer;
}
.ssoButton:hover {
  background-color: $primary-90;
}
.oauthLogin {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.75rem;
  margin-bottom: 1.5rem;
}
.oauthButton {
  background-color: $secondary;
  color: $text;
  padding: 0.75rem 1.5rem;
  border: none;
  border-radius: 0.5rem;
  font-size: 1.05rem;
  cursor: pointer;
}
.oauthButton:hover {
  background-color: $secondary-70;
}
.ssoHelp {
  margin-top: 0.5rem;
  font-size: 0.9rem;
  color: $text-50;
  text-align: center;
}
.separator {
  margin: 1.5rem 0;
  display: flex;
  align-items: center;
  color: $text-50;
  gap: 0.5rem;
  &::before,
  &::after {
    content: "";
    flex: 1;
    height: 1px;
    background: $text-50;
  }
}
.forgotPassword {
  text-align: right;
  color: $text;
  a {
    color: $text;
    text-decoration: none;
  }
}
</style>
