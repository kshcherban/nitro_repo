<template>
<v-container
    class="login-settings py-6">
    <v-row justify="center">
      <v-col cols="12" md="8" lg="6">
        <v-card>
          <v-card-title class="text-h6">Login Settings</v-card-title>
          <v-card-text>
            <div
              v-if="loading"
              class="text-center py-8">
              <v-progress-circular 
                indeterminate 
                color="primary"
                size="48" />
              <div class="mt-4 text-medium-emphasis">Loading password rules…</div>
            </div>

            <div
              v-else-if="!user"
              class="text-body-2 text-medium-emphasis">
              You must be signed in to change your password.
            </div>

            <div
              v-else-if="!passwordRules"
              class="text-body-2 text-medium-emphasis">
              Password changes are currently disabled by the administrator.
            </div>

            <form
              v-else
              @submit.prevent="changePassword">
              <input
                id="email"
                type="hidden"
                name="email"
                autocomplete="email"
                :value="user.email" />
              <input
                id="username"
                type="hidden"
                name="username"
                autocomplete="username"
                :value="user.username" />

              <v-alert
                v-if="feedback"
                :type="feedback.type"
                variant="tonal"
                border="start"
                density="comfortable"
                class="mb-4"
                closable
                @click:close="feedback = null">
                <div class="text-subtitle-1 font-weight-medium mb-1">
                  {{ feedback.title }}
                </div>
                <div v-if="feedback.message">
                  {{ feedback.message }}
                </div>
              </v-alert>

              <PasswordInput
                id="currentPassword"
                v-model="oldPassword"
                label="Current Password" />

              <NewPasswordInput
                id="newPassword"
                v-model="newPassword"
                :passwordRules="passwordRules">
                New Password
              </NewPasswordInput>

              <div class="d-flex justify-end mt-6">
                <SubmitButton
                  :block="false"
                  :disabled="!canSubmit || isSubmitting"
                  :loading="isSubmitting"
                  prepend-icon="mdi-content-save">
                  Change Password
                </SubmitButton>
              </div>
            </form>
          </v-card-text>
        </v-card>
      </v-col>
    </v-row>
  </v-container>
</template>
<script setup lang="ts">
import SubmitButton from "@/components/form/SubmitButton.vue";
import NewPasswordInput from "@/components/form/text/NewPasswordInput.vue";
import PasswordInput from "@/components/form/text/PasswordInput.vue";
import http from "@/http";
import { sessionStore } from "@/stores/session";
import { siteStore } from "@/stores/site";
import type { AxiosError } from "axios";
import { computed, onMounted, ref } from "vue";
const site = siteStore();
const session = sessionStore();
const user = session.user;
const oldPassword = ref("");
const newPassword = ref("");
const isSubmitting = ref(false);
const loading = ref(false);
const feedback = ref<{ type: "success" | "error"; title: string; message?: string } | null>(
  null,
);

onMounted(async () => {
  if (!site.siteInfo) {
    loading.value = true;
    try {
      await site.getInfo?.();
    } finally {
      loading.value = false;
    }
  }
});

const passwordRules = computed(() => site.siteInfo?.password_rules);

const canSubmit = computed(() => {
  if (isSubmitting.value) {
    return false;
  }
  const current = oldPassword.value.trim().length > 0;
  const next = (newPassword.value ?? "").trim().length > 0;
  if (!current || !next) {
    return false;
  }
  return true;
});

async function changePassword() {
  if (!user) {
    return;
  }
  if (!canSubmit.value) {
    return;
  }
  feedback.value = null;
  const request = {
    old_password: oldPassword.value,
    new_password: newPassword.value,
  };
  isSubmitting.value = true;
  try {
    await http.post("/api/user/change-password", request);
    oldPassword.value = "";
    newPassword.value = "";
    feedback.value = {
      type: "success",
      title: "Password updated",
      message: "Your password was changed successfully.",
    };
  } catch (error) {
    console.error(error);
    let message = "Failed to change password.";
    if (isAxiosError(error)) {
      const responseText = typeof error.response?.data === "string" ? error.response.data : undefined;
      message = responseText ?? message;
    }
    feedback.value = {
      type: "error",
      title: "Password update failed",
      message,
    };
  } finally {
    isSubmitting.value = false;
  }
}

function isAxiosError(error: unknown): error is AxiosError {
  return Boolean(error) && typeof error === "object" && "isAxiosError" in (error as any);
}
</script>

<style scoped lang="scss">
.login-settings {
  max-width: 960px;
}
</style>
