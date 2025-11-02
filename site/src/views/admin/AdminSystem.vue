<script setup lang="ts">
import SwitchInput from "@/components/form/SwitchInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import SpinnerElement from "@/components/spinner/SpinnerElement.vue";
import http from "@/http";
import { siteStore } from "@/stores/site";
import type { SsoConfiguration } from "@/types/base";
import { notify } from "@kyvg/vue3-notification";
import { computed, onMounted, ref } from "vue";

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

const loading = ref(true);
const saving = ref(false);
const form = ref<EditableSsoConfiguration>(defaultForm());
const initialSignature = ref<string>(JSON.stringify(toPayload(form.value)));
const site = siteStore();

const providerConfigured = computed(() => form.value.provider_login_url.trim().length > 0);
const hasChanges = computed(() => initialSignature.value !== JSON.stringify(toPayload(form.value)));

onMounted(async () => {
  await fetchSsoSettings();
});

async function fetchSsoSettings() {
  loading.value = true;
  try {
    const response = await http.get<SsoConfiguration>("/api/security/sso");
    form.value = toEditable(response.data);
    initialSignature.value = JSON.stringify(toPayload(form.value));
  } catch (error) {
    console.error("Failed to load SSO configuration", error);
    notify({
      type: "error",
      title: "Unable to load SSO settings",
      text: "Check the server logs for more information.",
    });
  } finally {
    loading.value = false;
  }
}

async function save() {
  if (saving.value) {
    return;
  }

  if (!form.value.username_header.trim()) {
    notify({
      type: "error",
      title: "Missing username header",
      text: "Username header cannot be empty.",
    });
    return;
  }

  saving.value = true;
  const payload = toPayload(form.value);

  try {
    await http.put("/api/security/sso", payload);
    notify({
      type: "success",
      title: "SSO settings updated",
    });
    form.value = toEditable(payload);
    initialSignature.value = JSON.stringify(toPayload(form.value));
    await site.getInfo();
  } catch (error: any) {
    console.error("Failed to update SSO configuration", error);
    notify({
      type: "error",
      title: "Unable to save SSO settings",
      text: error?.response?.data ?? "Check the server logs for more details.",
    });
  } finally {
    saving.value = false;
  }
}

function reset() {
  if (saving.value) {
    return;
  }
  const latest = JSON.parse(initialSignature.value) as SsoConfiguration;
  form.value = toEditable(latest);
}

function defaultForm(): EditableSsoConfiguration {
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

function toEditable(settings: SsoConfiguration): EditableSsoConfiguration {
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

function toPayload(settings: EditableSsoConfiguration): SsoConfiguration {
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
</script>

<template>
  <main class="systemSettings">
    <h1>System Settings</h1>
    <section class="card">
      <header>
        <h2>Single Sign-On</h2>
        <p>
          Configure how Nitro Repo integrates with your identity provider. Changes apply immediately
          and will affect the login screen.
        </p>
      </header>
      <SpinnerElement v-if="loading" />
      <form
        v-else
        class="ssoForm"
        @submit.prevent="save">
        <SwitchInput
          id="sso-enabled"
          v-model="form.enabled">
          Enable SSO
          <template #comment>
            When disabled, Nitro Repo hides the SSO button but keeps your saved configuration.
          </template>
        </SwitchInput>

        <div class="grid">
          <TextInput
            id="sso-login-path"
            v-model="form.login_path"
            autocomplete="off"
            required>
            Nitro Repo SSO endpoint
          </TextInput>

          <TextInput
            id="sso-button-text"
            v-model="form.login_button_text"
            autocomplete="off"
            required>
            Login button text
          </TextInput>

          <TextInput
            id="sso-provider-url"
            v-model="form.provider_login_url"
            autocomplete="off"
            placeholder="https://example.com/login">
            Identity provider login URL
          </TextInput>

          <TextInput
            id="sso-provider-param"
            v-model="form.provider_redirect_param"
            :disabled="!providerConfigured"
            autocomplete="off"
            placeholder="redirect_url">
            Provider redirect parameter
          </TextInput>

          <TextInput
            id="sso-username-header"
            v-model="form.username_header"
            autocomplete="off"
            required>
            Username header
          </TextInput>

          <TextInput
            id="sso-email-header"
            v-model="form.email_header"
            autocomplete="off"
            placeholder="CF-Access-Authenticated-User-Email">
            Email header (optional)
          </TextInput>

          <TextInput
            id="sso-display-header"
            v-model="form.display_name_header"
            autocomplete="off"
            placeholder="CF-Access-Authenticated-User-Name">
            Display name header (optional)
          </TextInput>
        </div>

        <SwitchInput
          id="sso-auto-create"
          v-model="form.auto_create_users">
          Auto-create users
          <template #comment>
            Create new Nitro Repo accounts automatically when someone signs in for the first time.
          </template>
        </SwitchInput>

        <footer class="actions">
          <SubmitButton
            :disabled="!hasChanges || saving"
            :loading="saving"
            title="Save SSO configuration">
            Save Changes
          </SubmitButton>
          <button
            class="secondary"
            type="button"
            :disabled="saving"
            @click="reset">
            Reset to Defaults
          </button>
        </footer>
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
