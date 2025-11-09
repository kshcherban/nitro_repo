<script setup lang="ts">
import SwitchInput from "@/components/form/SwitchInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import SpinnerElement from "@/components/spinner/SpinnerElement.vue";
import FloatingErrorBanner from "@/components/ui/FloatingErrorBanner.vue";
import http from "@/http";
import { siteStore } from "@/stores/site";
import type {
  OAuth2CasbinConfig,
  OAuth2Configuration,
  OAuth2GroupRoleMapping,
  OAuth2ProviderKind,
  SsoConfiguration,
} from "@/types/base";
import { notify } from "@kyvg/vue3-notification";
import { computed, onMounted, ref, watch } from "vue";
import { isAxiosError } from "axios";

interface EditableSsoConfiguration {
  enabled: boolean;
  login_path: string;
  login_button_text: string;
  provider_login_url: string;
  provider_redirect_param: string;
  username_header: string;
  email_header: string;
  display_name_header: string;
  auto_create_users: boolean;
}

interface EditableOAuthProvider {
  enabled: boolean;
  client_id: string;
  client_secret: string;
  secretConfigured: boolean;
  scopes: string;
  redirect_path: string;
  tenant_id?: string;
}

interface EditableGroupRoleMapping {
  id: string;
  provider: OAuth2ProviderKind;
  group: string;
  rolesText: string;
}

interface EditableOAuthConfiguration {
  enabled: boolean;
  login_path: string;
  callback_path: string;
  redirect_base_url: string;
  auto_create_users: boolean;
  google: EditableOAuthProvider;
  microsoft: EditableOAuthProvider;
  casbin_model: string;
  casbin_policy: string;
  group_role_mappings: EditableGroupRoleMapping[];
}

interface OAuth2ProviderUpdatePayload {
  client_id: string;
  client_secret: string | null;
  scopes: string[];
  redirect_path: string | null;
  tenant_id?: string | null;
}

interface OAuth2UpdatePayload {
  enabled: boolean;
  login_path: string;
  callback_path: string;
  redirect_base_url: string | null;
  auto_create_users: boolean;
  google: OAuth2ProviderUpdatePayload | null;
  microsoft: OAuth2ProviderUpdatePayload | null;
  casbin: OAuth2CasbinConfig | null;
  group_role_mappings: OAuth2GroupRoleMapping[];
}

const site = siteStore();

const ssoLoading = ref(true);
const ssoSaving = ref(false);
const ssoForm = ref<EditableSsoConfiguration>(defaultSsoForm());
const ssoInitialSignature = ref(JSON.stringify(toSsoPayload(ssoForm.value)));

const oauthLoading = ref(true);
const oauthSaving = ref(false);
const oauthForm = ref<EditableOAuthConfiguration>(defaultOAuthForm());
const oauthInitialSignature = ref(JSON.stringify(oauthForm.value));
const availableRoles = ref<string[]>([]);
const roleOptionsId = "nitro-role-options";

const providerConfigured = computed(() => ssoForm.value.provider_login_url.trim().length > 0);
const hasSsoChanges = computed(
  () => ssoInitialSignature.value !== JSON.stringify(toSsoPayload(ssoForm.value)),
);
const hasOAuthChanges = computed(
  () => oauthInitialSignature.value !== JSON.stringify(oauthForm.value),
);

const errorBanner = ref({
  visible: false,
  title: "",
  message: "",
});

const showError = (title: string, message: string) => {
  errorBanner.value.visible = true;
  errorBanner.value.title = title;
  errorBanner.value.message = message;
};

const resetError = () => {
  errorBanner.value.visible = false;
  errorBanner.value.title = "";
  errorBanner.value.message = "";
};

onMounted(async () => {
  await Promise.all([fetchSsoSettings(), fetchOAuthSettings()]);
});

watch(
  ssoForm,
  () => {
    if (errorBanner.value.visible) {
      resetError();
    }
  },
  { deep: true },
);

watch(
  oauthForm,
  () => {
    if (errorBanner.value.visible) {
      resetError();
    }
  },
  { deep: true },
);

function generateId(): string {
  if (typeof crypto !== "undefined" && crypto.randomUUID) {
    return crypto.randomUUID();
  }
  return Math.random().toString(36).slice(2);
}

async function fetchSsoSettings() {
  ssoLoading.value = true;
  resetError();
  try {
    const response = await http.get<SsoConfiguration>("/api/security/sso");
    ssoForm.value = toSsoEditable(response.data);
    ssoInitialSignature.value = JSON.stringify(toSsoPayload(ssoForm.value));
  } catch (error) {
    const resolved = resolveRequestError(
      error,
      "Unable to load SSO settings",
      "Check the server logs for more information.",
    );
    console.error(resolved.debugMessage);
    showError(resolved.title, resolved.message);
  } finally {
    ssoLoading.value = false;
  }
}

async function fetchOAuthSettings() {
  oauthLoading.value = true;
  resetError();
  try {
    const response = await http.get<OAuth2Configuration>("/api/security/oauth2");
    oauthForm.value = toOAuthEditable(response.data);
    oauthInitialSignature.value = JSON.stringify(oauthForm.value);
    availableRoles.value = response.data.available_roles ?? [];
  } catch (error) {
    const resolved = resolveRequestError(
      error,
      "Unable to load OAuth2 settings",
      "Check the server logs for more information.",
    );
    console.error(resolved.debugMessage);
    showError(resolved.title, resolved.message);
  } finally {
    oauthLoading.value = false;
  }
}

async function saveSsoSettings() {
  if (ssoSaving.value) {
    return;
  }
  resetError();
  if (!ssoForm.value.username_header.trim()) {
    showError("Missing username header", "Username header cannot be empty.");
    return;
  }

  ssoSaving.value = true;
  const payload = toSsoPayload(ssoForm.value);

  try {
    await http.put("/api/security/sso", payload);
    notify({
      type: "success",
      title: "SSO settings updated",
    });
    ssoForm.value = toSsoEditable(payload);
    ssoInitialSignature.value = JSON.stringify(toSsoPayload(ssoForm.value));
    await site.getInfo();
  } catch (error: any) {
    const resolved = resolveRequestError(
      error,
      "Unable to save SSO settings",
      "Check the server logs for more details.",
    );
    console.error(resolved.debugMessage);
    showError(resolved.title, resolved.message);
  } finally {
    ssoSaving.value = false;
  }
}

async function saveOAuthSettings() {
  if (oauthSaving.value) {
    return;
  }
  resetError();

  if (oauthForm.value.enabled) {
    if (oauthForm.value.google.enabled && !oauthForm.value.google.client_id.trim()) {
      showError(
        "Google client ID required",
        "Enter a Google client ID or disable the provider.",
      );
      return;
    }
    if (
      oauthForm.value.google.enabled &&
      !oauthForm.value.google.secretConfigured &&
      !oauthForm.value.google.client_secret.trim()
    ) {
      showError("Google client secret required", "Provide the Google client secret.");
      return;
    }
    if (oauthForm.value.microsoft.enabled && !oauthForm.value.microsoft.client_id.trim()) {
      showError(
        "Microsoft client ID required",
        "Enter a Microsoft client ID or disable the provider.",
      );
      return;
    }
    if (
      oauthForm.value.microsoft.enabled &&
      !oauthForm.value.microsoft.secretConfigured &&
      !oauthForm.value.microsoft.client_secret.trim()
    ) {
      showError(
        "Microsoft client secret required",
        "Provide the Microsoft client secret.",
      );
      return;
    }
  }

  oauthSaving.value = true;
  const payload = toOAuthPayload(oauthForm.value);

  try {
    await http.put("/api/security/oauth2", payload);
    notify({
      type: "success",
      title: "OAuth2 settings updated",
    });
    await fetchOAuthSettings();
    await site.getInfo();
  } catch (error: any) {
    const resolved = resolveRequestError(
      error,
      "Unable to save OAuth2 settings",
      "Check the server logs for more details.",
    );
    console.error(resolved.debugMessage);
    showError(resolved.title, resolved.message);
  } finally {
    oauthSaving.value = false;
  }
}

function resetSsoSettings() {
  if (ssoSaving.value) {
    return;
  }
  resetError();
  const latest = JSON.parse(ssoInitialSignature.value) as SsoConfiguration;
  ssoForm.value = toSsoEditable(latest);
}

function resetOAuthSettings() {
  if (oauthSaving.value) {
    return;
  }
  resetError();
  const latest = JSON.parse(oauthInitialSignature.value) as EditableOAuthConfiguration;
  oauthForm.value = latest;
}

function defaultSsoForm(): EditableSsoConfiguration {
  return {
    enabled: false,
    login_path: "/api/user/sso/login",
    login_button_text: "Sign in with SSO",
    provider_login_url: "",
    provider_redirect_param: "redirect",
    username_header: "X-Forwarded-User",
    email_header: "X-Forwarded-Email",
    display_name_header: "X-Forwarded-Name",
    auto_create_users: false,
  };
}

function defaultProvider(): EditableOAuthProvider {
  return {
    enabled: false,
    client_id: "",
    client_secret: "",
    secretConfigured: false,
    scopes: "openid profile email",
    redirect_path: "",
    tenant_id: "",
  };
}

function defaultOAuthForm(): EditableOAuthConfiguration {
  return {
    enabled: false,
    login_path: "/api/user/oauth2/login",
    callback_path: "/api/user/oauth2/callback",
    redirect_base_url: "",
    auto_create_users: false,
    google: defaultProvider(),
    microsoft: defaultProvider(),
    casbin_model: "",
    casbin_policy: "",
    group_role_mappings: [],
  };
}

function resolveRequestError(
  error: unknown,
  fallbackTitle: string,
  fallbackMessage: string,
  conflictTitle?: string,
  conflictMessage?: string,
): { title: string; message: string; debugMessage: string } {
  const fallback = {
    title: fallbackTitle,
    message: fallbackMessage,
    debugMessage: typeof error === "string" ? error : JSON.stringify(error),
  };

  if (isAxiosError(error)) {
    const status = error.response?.status;
    const data = error.response?.data;
    let payloadMessage: string | undefined;

    if (typeof data === "string" && data.trim().length > 0) {
      payloadMessage = data.trim();
    } else if (typeof data === "object" && data !== null && "message" in data) {
      const candidate = (data as { message?: unknown }).message;
      if (typeof candidate === "string" && candidate.trim().length > 0) {
        payloadMessage = candidate.trim();
      }
    }

    if (status === 409 && conflictTitle) {
      return {
        title: conflictTitle,
        message: conflictMessage ?? payloadMessage ?? fallbackMessage,
        debugMessage: JSON.stringify(error.toJSON?.() ?? error),
      };
    }

    if (payloadMessage) {
      return {
        title: fallbackTitle,
        message: payloadMessage,
        debugMessage: JSON.stringify(error.toJSON?.() ?? error),
      };
    }

    return {
      title: fallbackTitle,
      message: `Request failed${status ? ` with status ${status}` : ""}.`,
      debugMessage: JSON.stringify(error.toJSON?.() ?? error),
    };
  }

  if (error instanceof Error) {
    return {
      title: fallbackTitle,
      message: error.message,
      debugMessage: error.stack ?? error.message,
    };
  }

  return fallback;
}

function toSsoEditable(settings: SsoConfiguration): EditableSsoConfiguration {
  return {
    enabled: settings.enabled,
    login_path: settings.login_path,
    login_button_text: settings.login_button_text,
    provider_login_url: settings.provider_login_url ?? "",
    provider_redirect_param: settings.provider_redirect_param ?? "redirect",
    username_header: settings.username_header,
    email_header: settings.email_header ?? "",
    display_name_header: settings.display_name_header ?? "",
    auto_create_users: settings.auto_create_users,
  };
}

function toSsoPayload(settings: EditableSsoConfiguration): SsoConfiguration {
  const sanitizeOptional = (value: string) => {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  };

  return {
    enabled: settings.enabled,
    login_path: settings.login_path.trim() || "/api/user/sso/login",
    login_button_text: settings.login_button_text.trim() || "Sign in with SSO",
    provider_login_url: sanitizeOptional(settings.provider_login_url),
    provider_redirect_param: sanitizeOptional(settings.provider_redirect_param ?? "redirect"),
    username_header: settings.username_header.trim() || "X-Forwarded-User",
    email_header: sanitizeOptional(settings.email_header),
    display_name_header: sanitizeOptional(settings.display_name_header),
    auto_create_users: settings.auto_create_users,
  };
}

function toOAuthEditable(settings: OAuth2Configuration): EditableOAuthConfiguration {
  const form = defaultOAuthForm();
  form.enabled = settings.enabled;
  form.login_path = settings.login_path;
  form.callback_path = settings.callback_path;
  form.redirect_base_url = settings.redirect_base_url ?? "";
  form.auto_create_users = settings.auto_create_users;

  if (settings.google) {
    form.google.enabled = true;
    form.google.client_id = settings.google.client_id;
    form.google.secretConfigured = settings.google.client_secret_configured;
    form.google.scopes = settings.google.scopes.join(" ");
    form.google.redirect_path = settings.google.redirect_path ?? "";
  }

  if (settings.microsoft) {
    form.microsoft.enabled = true;
    form.microsoft.client_id = settings.microsoft.client_id;
    form.microsoft.secretConfigured = settings.microsoft.client_secret_configured;
    form.microsoft.scopes = settings.microsoft.scopes.join(" ");
    form.microsoft.redirect_path = settings.microsoft.redirect_path ?? "";
    form.microsoft.tenant_id = settings.microsoft.tenant_id ?? "";
  }

  form.casbin_model = settings.casbin?.model ?? "";
  form.casbin_policy = settings.casbin?.policy ?? "";

  availableRoles.value = settings.available_roles ?? [];

  form.group_role_mappings = settings.group_role_mappings.map((mapping) => ({
    id: generateId(),
    provider: mapping.provider,
    group: mapping.group,
    rolesText: mapping.roles.join(", "),
  }));

  return form;
}

function toOAuthPayload(settings: EditableOAuthConfiguration): OAuth2UpdatePayload {
  const sanitizeOptional = (value: string) => {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  };

  const sanitizeScopes = (value: string) => {
    const tokens = value.split(/[\s,]+/).map((scope) => scope.trim()).filter((scope) => scope);
    return tokens.length > 0 ? tokens : ["openid", "profile", "email"];
  };

  const google = settings.google.enabled
    ? {
        client_id: settings.google.client_id.trim(),
        client_secret: settings.google.client_secret.trim()
          ? settings.google.client_secret.trim()
          : settings.google.secretConfigured
            ? null
            : null,
        scopes: sanitizeScopes(settings.google.scopes),
        redirect_path: sanitizeOptional(settings.google.redirect_path),
      }
    : null;

  const microsoft = settings.microsoft.enabled
    ? {
        client_id: settings.microsoft.client_id.trim(),
        client_secret: settings.microsoft.client_secret.trim()
          ? settings.microsoft.client_secret.trim()
          : settings.microsoft.secretConfigured
            ? null
            : null,
        scopes: sanitizeScopes(settings.microsoft.scopes),
        redirect_path: sanitizeOptional(settings.microsoft.redirect_path),
        tenant_id: sanitizeOptional(settings.microsoft.tenant_id ?? ""),
      }
    : null;

  const filteredMappings: OAuth2GroupRoleMapping[] = settings.group_role_mappings
    .map((mapping) => {
      const roles = mapping.rolesText
        .split(/[\s,]+/)
        .map((role) => role.trim())
        .filter((role) => role);
      return {
        provider: mapping.provider,
        group: mapping.group.trim(),
        roles,
      };
    })
    .filter((mapping) => mapping.group.length > 0 && mapping.roles.length > 0)
    .map((mapping) => ({
      ...mapping,
      roles: Array.from(new Set(mapping.roles)),
    }));

  return {
    enabled: settings.enabled,
    login_path: settings.login_path.trim() || "/api/user/oauth2/login",
    callback_path: settings.callback_path.trim() || "/api/user/oauth2/callback",
    redirect_base_url: sanitizeOptional(settings.redirect_base_url),
    auto_create_users: settings.auto_create_users,
    google,
    microsoft,
    casbin:
      settings.casbin_model.trim() || settings.casbin_policy.trim()
        ? {
            model: settings.casbin_model.trim(),
            policy: settings.casbin_policy.trim(),
          }
        : null,
    group_role_mappings: filteredMappings,
  };
}

function addGroupMapping() {
  oauthForm.value.group_role_mappings.push({
    id: generateId(),
    provider: "microsoft",
    group: "",
    rolesText: "",
  });
}

function removeGroupMapping(id: string) {
  oauthForm.value.group_role_mappings = oauthForm.value.group_role_mappings.filter(
    (mapping) => mapping.id !== id,
  );
}
</script>

<template>
  <main class="systemSettings">
    <FloatingErrorBanner
      :visible="errorBanner.visible"
      :title="errorBanner.title"
      :message="errorBanner.message"
      @close="resetError" />
    <h1>System Settings</h1>

    <section class="card">
      <header>
        <h2>Single Sign-On</h2>
        <p>
          Configure how Nitro Repo integrates with an upstream SSO proxy. Changes apply immediately
          and will affect the login screen.
        </p>
      </header>
      <SpinnerElement v-if="ssoLoading" />
      <form
        v-else
        class="ssoForm"
        @submit.prevent="saveSsoSettings">
        <SwitchInput
          id="sso-enabled"
          v-model="ssoForm.enabled">
          Enable SSO
          <template #comment>
            When disabled, Nitro Repo hides the SSO button but keeps your saved configuration.
          </template>
        </SwitchInput>

        <div class="grid">
          <TextInput
            id="sso-login-path"
            v-model="ssoForm.login_path"
            autocomplete="off"
            required>
            Nitro Repo SSO endpoint
          </TextInput>

          <TextInput
            id="sso-button-text"
            v-model="ssoForm.login_button_text"
            autocomplete="off"
            required>
            Login button text
          </TextInput>

          <TextInput
            id="sso-provider-url"
            v-model="ssoForm.provider_login_url"
            autocomplete="off"
            placeholder="https://example.com/login">
            Identity provider login URL
          </TextInput>

          <TextInput
            id="sso-provider-param"
            v-model="ssoForm.provider_redirect_param"
            :disabled="!providerConfigured"
            autocomplete="off"
            placeholder="redirect_url">
            Provider redirect parameter
          </TextInput>

          <TextInput
            id="sso-username-header"
            v-model="ssoForm.username_header"
            autocomplete="off"
            required>
            Username header
          </TextInput>

          <TextInput
            id="sso-email-header"
            v-model="ssoForm.email_header"
            autocomplete="off"
            placeholder="CF-Access-Authenticated-User-Email">
            Email header (optional)
          </TextInput>

          <TextInput
            id="sso-display-header"
            v-model="ssoForm.display_name_header"
            autocomplete="off"
            placeholder="CF-Access-Authenticated-User-Name">
            Display name header (optional)
          </TextInput>
        </div>

        <SwitchInput
          id="sso-auto-create"
          v-model="ssoForm.auto_create_users">
          Auto-create users
          <template #comment>
            Create new Nitro Repo accounts automatically when someone signs in for the first time.
          </template>
        </SwitchInput>

        <footer class="actions">
          <SubmitButton
            :disabled="!hasSsoChanges || ssoSaving"
            :loading="ssoSaving"
            title="Save SSO configuration">
            Save Changes
          </SubmitButton>
          <button
            class="secondary"
            type="button"
            :disabled="ssoSaving"
            @click="resetSsoSettings">
            Reset to Defaults
          </button>
        </footer>
      </form>
    </section>

    <section class="card">
      <header>
        <h2>OAuth2 Providers</h2>
        <p>
          Configure direct Google or Microsoft sign-in and map external groups to Nitro Repo roles.
        </p>
      </header>
      <SpinnerElement v-if="oauthLoading" />
      <form
        v-else
        class="oauthForm"
        @submit.prevent="saveOAuthSettings">
        <SwitchInput
          id="oauth-enabled"
          v-model="oauthForm.enabled">
          Enable OAuth2
          <template #comment>
            When disabled, provider-specific buttons are hidden from the login screen.
          </template>
        </SwitchInput>

        <div class="grid">
          <TextInput
            id="oauth-login-path"
            v-model="oauthForm.login_path"
            autocomplete="off"
            required>
            OAuth2 login endpoint
          </TextInput>

          <TextInput
            id="oauth-callback-path"
            v-model="oauthForm.callback_path"
            autocomplete="off"
            required>
            OAuth2 callback endpoint
          </TextInput>

          <TextInput
            id="oauth-base-url"
            v-model="oauthForm.redirect_base_url"
            autocomplete="off"
            placeholder="https://repo.example.com">
            Redirect base URL (optional)
          </TextInput>
        </div>

        <SwitchInput
          id="oauth-auto-create"
          v-model="oauthForm.auto_create_users">
          Auto-create users
          <template #comment>
            Provision Nitro Repo accounts the first time someone signs in via OAuth2.
          </template>
        </SwitchInput>

        <div class="casbinEditors">
          <label
            class="textareaLabel"
            for="casbin-model">
            Casbin model
            <textarea
              id="casbin-model"
              v-model="oauthForm.casbin_model"
              :disabled="!oauthForm.enabled"
              rows="8" />
          </label>
          <label
            class="textareaLabel"
            for="casbin-policy">
            Casbin policy
            <textarea
              id="casbin-policy"
              v-model="oauthForm.casbin_policy"
              :disabled="!oauthForm.enabled"
              rows="8" />
          </label>
        </div>

        <div class="providerSection">
          <header>
            <h3>Google</h3>
            <SwitchInput
              id="google-enabled"
              v-model="oauthForm.google.enabled"
              :disabled="!oauthForm.enabled">
              Enable Google login
            </SwitchInput>
          </header>
          <div class="grid">
            <TextInput
              id="google-client-id"
              v-model="oauthForm.google.client_id"
              :disabled="!oauthForm.enabled || !oauthForm.google.enabled"
              autocomplete="off">
              Client ID
            </TextInput>
            <TextInput
              id="google-client-secret"
              v-model="oauthForm.google.client_secret"
              :disabled="!oauthForm.enabled || !oauthForm.google.enabled"
              type="password"
              autocomplete="off"
              placeholder="Leave blank to keep existing"
            >
              Client secret
            </TextInput>
            <TextInput
              id="google-scopes"
              v-model="oauthForm.google.scopes"
              :disabled="!oauthForm.enabled || !oauthForm.google.enabled"
              autocomplete="off">
              Scopes (space or comma separated)
            </TextInput>
            <TextInput
              id="google-redirect"
              v-model="oauthForm.google.redirect_path"
              :disabled="!oauthForm.enabled || !oauthForm.google.enabled"
              autocomplete="off"
              placeholder="/api/user/oauth2/callback">
              Redirect path override (optional)
            </TextInput>
          </div>
          <p
            v-if="oauthForm.google.secretConfigured && oauthForm.enabled"
            class="hint">
            Secret stored on server. Provide a new value to rotate it.
          </p>
        </div>

        <div class="providerSection">
          <header>
            <h3>Microsoft Entra ID</h3>
            <SwitchInput
              id="microsoft-enabled"
              v-model="oauthForm.microsoft.enabled"
              :disabled="!oauthForm.enabled">
              Enable Microsoft login
            </SwitchInput>
          </header>
          <div class="grid">
            <TextInput
              id="microsoft-client-id"
              v-model="oauthForm.microsoft.client_id"
              :disabled="!oauthForm.enabled || !oauthForm.microsoft.enabled"
              autocomplete="off">
              Client ID
            </TextInput>
            <TextInput
              id="microsoft-client-secret"
              v-model="oauthForm.microsoft.client_secret"
              :disabled="!oauthForm.enabled || !oauthForm.microsoft.enabled"
              type="password"
              autocomplete="off"
              placeholder="Leave blank to keep existing"
            >
              Client secret
            </TextInput>
            <TextInput
              id="microsoft-tenant-id"
              v-model="oauthForm.microsoft.tenant_id"
              :disabled="!oauthForm.enabled || !oauthForm.microsoft.enabled"
              autocomplete="off"
              placeholder="common">
              Tenant ID (optional)
            </TextInput>
            <TextInput
              id="microsoft-scopes"
              v-model="oauthForm.microsoft.scopes"
              :disabled="!oauthForm.enabled || !oauthForm.microsoft.enabled"
              autocomplete="off">
              Scopes (space or comma separated)
            </TextInput>
            <TextInput
              id="microsoft-redirect"
              v-model="oauthForm.microsoft.redirect_path"
              :disabled="!oauthForm.enabled || !oauthForm.microsoft.enabled"
              autocomplete="off"
              placeholder="/api/user/oauth2/callback">
              Redirect path override (optional)
            </TextInput>
          </div>
          <p
            v-if="oauthForm.microsoft.secretConfigured && oauthForm.enabled"
            class="hint">
            Secret stored on server. Provide a new value to rotate it.
          </p>
        </div>

        <div class="roleMappings">
          <header class="roleMappings__header">
            <h3>Group to role mappings</h3>
            <p>
              Map provider group or role claims to Nitro Repo Casbin roles. Roles are comma
              separated.
            </p>
          </header>
          <div
            v-if="oauthForm.group_role_mappings.length === 0"
            class="emptyState">
            No mappings configured yet.
          </div>
          <div
            v-for="mapping in oauthForm.group_role_mappings"
            :key="mapping.id"
            class="mappingRow">
            <label>
              Provider
              <select v-model="mapping.provider">
                <option value="google">Google</option>
                <option value="microsoft">Microsoft</option>
              </select>
            </label>
            <TextInput
              :id="`mapping-group-${mapping.id}`"
              v-model="mapping.group"
              autocomplete="off"
              required>
              Group or role ID
            </TextInput>
            <TextInput
              :id="`mapping-roles-${mapping.id}`"
              v-model="mapping.rolesText"
              :list="roleOptionsId"
              autocomplete="off"
              placeholder="read/write, admin">
              Nitro roles (comma separated)
            </TextInput>
            <button
              type="button"
              class="secondary"
              @click="removeGroupMapping(mapping.id)">
              Remove
            </button>
          </div>
          <button
            type="button"
            class="secondary"
            @click="addGroupMapping">
            Add mapping
          </button>
        </div>

        <footer class="actions">
          <SubmitButton
            :disabled="!hasOAuthChanges || oauthSaving"
            :loading="oauthSaving"
            title="Save OAuth2 configuration">
            Save Changes
          </SubmitButton>
          <button
            class="secondary"
            type="button"
            :disabled="oauthSaving"
            @click="resetOAuthSettings">
            Reset to Saved Values
          </button>
        </footer>
        <datalist :id="roleOptionsId">
          <option
            v-for="role in availableRoles"
            :key="role"
            :value="role" />
        </datalist>
      </form>
    </section>
  </main>
</template>

<style scoped lang="scss">
@import "@/assets/styles/theme.scss";

.systemSettings {
  max-width: 900px;
  margin: 0 auto;
  padding: 2rem 1rem;

  h1 {
    margin-bottom: 1rem;
    text-align: center;
  }
}

.card {
  background: $background-70;
  border-radius: 1rem;
  padding: 1.5rem;
  box-shadow: 0 0 10px rgba(0, 0, 0, 0.25);
}

.ssoForm {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.oauthForm {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 1rem 1.5rem;
}

.actions {
  display: flex;
  gap: 1rem;
  align-items: center;
}

.actions .secondary {
  background: transparent;
  color: $text;
  border: 1px solid $text-50;
  border-radius: 0.5rem;
  padding: 0.75rem 1.5rem;
  cursor: pointer;
}

.actions .secondary:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.providerSection {
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 0.75rem;
  padding: 1rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  background: $background-70;
}

.casbinEditors {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 1rem 1.5rem;
}

.textareaLabel {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.textareaLabel textarea {
  width: 100%;
  min-height: 200px;
  padding: 0.75rem;
  border-radius: 0.5rem;
  border: 1px solid rgba(255, 255, 255, 0.15);
  background: $background-70;
  color: $text;
  font-family: inherit;
  resize: vertical;
}

.providerSection header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.providerSection h3 {
  margin: 0;
}

.hint {
  color: $text-50;
  font-size: 0.9rem;
  margin: 0;
}

.roleMappings {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.roleMappings__header {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.roleMappings__header h3 {
  margin: 0;
}

.mappingRow {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 1rem;
  align-items: end;
}

.mappingRow select {
  width: 100%;
  padding: 0.75rem;
  border-radius: 0.5rem;
  border: 1px solid rgba(255, 255, 255, 0.15);
  background: $background-70;
  color: $text;
}

.emptyState {
  padding: 0.75rem 1rem;
  border-radius: 0.5rem;
  background: rgba(255, 255, 255, 0.05);
  color: $text-50;
}

.actions .secondary:not(:disabled):hover {
  background: $text-50;
  color: $background;
}

header p {
  margin-top: 0.5rem;
  color: $text-50;
}

@media (max-width: 600px) {
  .systemSettings {
    padding: 1rem 0.5rem;
  }
  .card {
    padding: 1rem;
  }
  .actions {
    flex-direction: column;
    align-items: stretch;
  }
}
</style>
