<template>
  <main>
    <FloatingErrorBanner
      :visible="errorBanner.visible"
      :title="errorBanner.title"
      :message="errorBanner.message"
      @close="resetError" />
    <h1>User Create</h1>
    <p>Create a new user here.</p>
    <form @submit.prevent="create">
      <TextInput
        v-model="user.name"
        :disabled="isSubmitting"
        required>
        Name
      </TextInput>
      <ValidatableTextBox
        id="email"
        type="email"
        :validations="EMAIL_VALIDATIONS"
        v-model="user.email"
        :disabled="isSubmitting"
        @validity="emailValid = $event">
        Email
      </ValidatableTextBox>
      <ValidatableTextBox
        id="username"
        :validations="USERNAME_VALIDATIONS"
        :deniedKeys="URL_SAFE_BAD_CHARS"
        v-model="user.username"
        :disabled="isSubmitting"
        @validity="usernameValid = $event">
        Username
      </ValidatableTextBox>
      <SwitchInput
        id="setPassword"
        v-model="setPassword"
        :disabled="isSubmitting"
        >Set Password</SwitchInput
      >
  <div v-if="setPassword">
    <NewPasswordInput
      id="password"
      :passwordRules="passwordRules"
      v-model="password"
      :disabled="isSubmitting" />
  </div>
      <SubmitButton
        class="primary-action"
        :disabled="!formIsValid || isSubmitting">
        <span v-if="isSubmitting">Creating…</span>
        <span v-else>Create User</span>
      </SubmitButton>
    </form>
  </main>
</template>
<script lang="ts" setup>
import SubmitButton from "@/components/form/SubmitButton.vue";
import SwitchInput from "@/components/form/SwitchInput.vue";
import NewPasswordInput from "@/components/form/text/NewPasswordInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import ValidatableTextBox from "@/components/form/text/ValidatableTextBox.vue";
import FloatingErrorBanner from "@/components/ui/FloatingErrorBanner.vue";
import {
  EMAIL_VALIDATIONS,
  URL_SAFE_BAD_CHARS,
  USERNAME_VALIDATIONS,
} from "@/components/form/text/validations";
import http from "@/http";
import router from "@/router";
import { siteStore } from "@/stores/site";
import type { PasswordRules } from "@/types/base";
import { isAxiosError } from "axios";
import { computed, watch, type Ref, ref } from "vue";
const user = ref({
  name: "",
  email: "",
  username: "",
});
const site = siteStore();
if (!site.siteInfo) {
  site.getInfo();
}
const passwordRules = computed(() => site.getPasswordRulesOrDefault());

const setPassword = ref(false);

const password: Ref<string | undefined> = ref(undefined);

const errorBanner = ref({
  visible: false,
  title: "",
  message: "",
});

const isSubmitting = ref(false);
const emailValid = ref(false);
const usernameValid = ref(false);

const resetError = () => {
  errorBanner.value.visible = false;
  errorBanner.value.title = "";
  errorBanner.value.message = "";
};

watch(
  () => [
    user.value.name,
    user.value.email,
    user.value.username,
    setPassword.value,
    password.value,
    emailValid.value,
    usernameValid.value,
  ],
  () => {
    if (errorBanner.value.visible) {
      resetError();
    }
  },
);

watch(setPassword, (enabled) => {
  if (!enabled) {
    password.value = undefined;
  }
});

const nameValid = computed(() => user.value.name.trim().length > 0);
const passwordValid = computed(() => !setPassword.value || !!password.value);
const formIsValid = computed(
  () => nameValid.value && emailValid.value && usernameValid.value && passwordValid.value,
);

async function create() {
  if (isSubmitting.value) {
    return;
  }

  if (!formIsValid.value) {
    errorBanner.value = {
      visible: true,
      title: "Review the form",
      message: "Please resolve validation errors before creating the user.",
    };
    return;
  }

  const requestBody = {
    name: user.value.name.trim(),
    email: user.value.email,
    username: user.value.username,
    password: password.value,
  };

  resetError();
  isSubmitting.value = true;
  try {
    await http.post("/api/user-management/create", requestBody);
    router.push("/admin/users");
  } catch (error) {
    const resolved = resolveUserCreateError(error);
    errorBanner.value = {
      visible: true,
      title: resolved.title,
      message: resolved.message,
    };
    console.error(resolved.debugMessage);
  } finally {
    isSubmitting.value = false;
  }
}

function resolveUserCreateError(error: unknown): {
  title: string;
  message: string;
  debugMessage: string;
} {
  const normalizeApiError = (
    data: unknown,
  ): { message?: string; details?: string | string[] } => {
    if (!data) {
      return {};
    }
    if (typeof data === "string") {
      return { message: data.trim() };
    }
    if (typeof data === "object") {
      const maybeMessage = (data as { message?: unknown }).message;
      const maybeDetails = (data as { details?: unknown }).details;
      return {
        message:
          typeof maybeMessage === "string" && maybeMessage.trim().length > 0
            ? maybeMessage.trim()
            : undefined,
        details:
          typeof maybeDetails === "string" || Array.isArray(maybeDetails)
            ? (maybeDetails as string | string[])
            : undefined,
      };
    }
    return {};
  };

  const fallback = {
    title: "Unable to create user",
    message: "An unexpected error occurred. Please review the form and try again.",
    debugMessage: typeof error === "string" ? error : JSON.stringify(error),
  };

  if (isAxiosError(error)) {
    const status = error.response?.status;
    const data = error.response?.data;
    const api = normalizeApiError(data);
    let payloadMessage = api.message;

    if (status === 400) {
      return {
        title: "Invalid user details",
        message:
          payloadMessage ??
          "Please ensure name, email, and username are present and valid, then try again.",
        debugMessage: JSON.stringify(error.toJSON?.() ?? error),
      };
    }

    if (status === 409) {
      const details = Array.isArray(api.details)
        ? api.details
        : api.details
          ? [api.details]
          : [];
      if (details.some((item) => item.toLowerCase().includes("username"))) {
        return {
          title: "Username already exists",
          message:
            payloadMessage ??
            "A user with this username already exists. Choose a different username.",
          debugMessage: JSON.stringify(error.toJSON?.() ?? error),
        };
      }
      if (details.some((item) => item.toLowerCase().includes("email"))) {
        return {
          title: "Email already exists",
          message:
            payloadMessage ??
            "A user with this email address already exists. Enter a different email.",
          debugMessage: JSON.stringify(error.toJSON?.() ?? error),
        };
      }
      return {
        title: "User already exists",
        message:
          payloadMessage ??
          "A user with the same credentials already exists. Adjust the username or email.",
        debugMessage: JSON.stringify(error.toJSON?.() ?? error),
      };
    }

    if (payloadMessage) {
      return {
        title: fallback.title,
        message: payloadMessage,
        debugMessage: JSON.stringify(error.toJSON?.() ?? error),
      };
    }

    return {
      title: fallback.title,
      message: `Request failed${status ? ` with status ${status}` : ""}.`,
      debugMessage: JSON.stringify(error.toJSON?.() ?? error),
    };
  }

  if (error instanceof Error) {
    return {
      title: fallback.title,
      message: error.message,
      debugMessage: error.stack ?? error.message,
    };
  }

  return fallback;
}
</script>

<style scoped lang="scss">
@import "@/assets/styles/theme.scss";

main {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
  max-width: 640px;
}

form {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

:deep(.primary-action) {
  width: auto;
  min-width: 160px;
  padding-inline: 1.5rem;
  align-self: flex-start;
}
</style>
