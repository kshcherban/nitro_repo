<template>
  <div
    v-if="user"
    class="admin-user-page">
    <FloatingErrorBanner
      :visible="errorBanner.visible"
      :title="errorBanner.title"
      :message="errorBanner.message"
      @close="resetError" />
    <div class="tabs">
      <div class="tabs-header">
      <div
        class="tab"
        :data-active="currentTab === 'main'"
        @click="currentTab = 'main'">
        User
      </div>
      <div
        class="tab"
        :data-active="currentTab === 'password'"
        @click="currentTab = 'password'">
        Password
      </div>
      <div
        class="tab"
        :data-active="currentTab === 'user-permissions'"
        @click="currentTab = 'user-permissions'">
        User Permissions
      </div>
      <div
        class="tab"
        :data-active="currentTab === 'repository-permissions'"
        @click="currentTab = 'repository-permissions'">
        Repository Permissions
      </div>
      </div>
      <div class="tabs-content">
        <div
          class="tab-content"
          :data-active="currentTab === 'main'">
          <div id="userMain">
            <div class="userStatus">
            <span
              class="statusBadge"
              :data-active="user.active">
              {{ user.active ? "Active" : "Inactive" }}
            </span>
            <div class="statusActions">
              <button
                type="button"
                class="secondaryButton"
                :disabled="statusUpdating"
                @click="setActive(!user.active)">
                {{ user.active ? "Deactivate" : "Reactivate" }}
              </button>
              <button
                type="button"
                class="dangerButton"
                :disabled="deletingUser || isCurrentUser"
                @click="deleteUser">
                Delete User
              </button>
            </div>
          </div>
          <form>
            <TextInput
              id="name"
              v-model="changeUser.name"
              autocomplete="name">
              Name</TextInput
            >
            <ValidatableTextBox
              id="email"
              autocomplete="email"
              :validations="EMAIL_VALIDATIONS"
              :originalValue="user.email"
              v-model="changeUser.email">
              Email
            </ValidatableTextBox>
            <ValidatableTextBox
              id="username"
              :originalValue="user.username"
              :validations="USERNAME_VALIDATIONS"
              :deniedKeys="[' ']"
              autocomplete="username"
              v-model="changeUser.username">
              Username
            </ValidatableTextBox>
            <SubmitButton>Save</SubmitButton>
          </form>
          <div>
            <KeyAndValue
              :label="'ID #'"
              :value="user.id.toLocaleString()" />
            <KeyAndValue
              :label="'Status'"
              :value="user.active ? 'Active' : 'Inactive'" />
            <KeyAndValue
              :label="'Created At'"
              :value="new Date(user.created_at).toLocaleString()" />
          </div>
        </div>
        </div>
        <div
          class="tab-content"
          :data-active="currentTab === 'password'">
          <form
            id="setPassword"
            @submit.prevent="changePassword">
            <input
            type="hidden"
            name="email"
            autocomplete="email"
            :value="user.email" />
            <input
            type="hidden"
            name="username"
            autocomplete="username"
            :value="user.username" />
            <NewPasswordInput
            id="password"
            v-model="newPassword"
            :passwordRules="passwordRules">
            Password</NewPasswordInput
          >
            <SubmitButton :disabled="!newPassword">Save</SubmitButton>
          </form>
        </div>
        <div
          class="tab-content"
          :data-active="currentTab === 'user-permissions'">
          <UserPermissions :user="user" />
        </div>
        <div
          class="tab-content"
          :data-active="currentTab === 'repository-permissions'">
          <RepositoryPermissions :user="user" />
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import KeyAndValue from "@/components/form/KeyAndValue.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import NewPasswordInput from "@/components/form/text/NewPasswordInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import { siteStore } from "@/stores/site";
import { sessionStore } from "@/stores/session";
import type { UserResponseType } from "@/types/base";
import FloatingErrorBanner from "@/components/ui/FloatingErrorBanner.vue";
import { computed, ref, type PropType, watch } from "vue";
import UserPermissions from "./UserPermissions.vue";
import RepositoryPermissions from "./RepositoryPermissions.vue";
import http from "@/http";
import { notify } from "@kyvg/vue3-notification";
import ValidatableTextBox from "@/components/form/text/ValidatableTextBox.vue";
import { EMAIL_VALIDATIONS, USERNAME_VALIDATIONS } from "@/components/form/text/validations";
import { isAxiosError } from "axios";
const props = defineProps({
  user: {
    type: Object as PropType<UserResponseType>,
    required: true,
  },
});
const emit = defineEmits<{
  (e: "refresh"): void;
  (e: "deleted"): void;
}>();
const currentTab = ref("main");
const changeUser = ref({
  name: "",
  email: "",
  username: "",
});
const newPassword = ref<string | undefined>(undefined);

const site = siteStore();
if (!site.siteInfo) {
  site.getInfo();
}
const passwordRules = computed(() => site.getPasswordRulesOrDefault());
const session = sessionStore();
const isCurrentUser = computed(() => session.user?.id === props.user.id);
const statusUpdating = ref(false);
const deletingUser = ref(false);
const errorBanner = ref({
  visible: false,
  title: "",
  message: "",
});

const resetError = () => {
  errorBanner.value.visible = false;
  errorBanner.value.title = "";
  errorBanner.value.message = "";
};

const showError = (title: string, message: string) => {
  errorBanner.value.visible = true;
  errorBanner.value.title = title;
  errorBanner.value.message = message;
};

watch(
  () => props.user,
  (newUser) => {
    if (!newUser) {
      return;
    }
    changeUser.value = {
      name: newUser.name,
      email: newUser.email,
      username: newUser.username,
    };
    resetError();
  },
  { immediate: true },
);

watch(newPassword, () => {
  if (errorBanner.value.visible) {
    resetError();
  }
});

async function changePassword() {
  console.log("Changing Password");

  if (!newPassword.value) {
    notify({
      type: "error",
      title: "Password required",
      text: "Enter and confirm a password before saving.",
    });
    return;
  }

  console.log("Password is valid");

  http
    .put(`/api/user-management/update/${props.user.id}/password`, {
      password: newPassword.value,
    })
    .then(() => {
      notify({
        type: "success",
        title: "Password Changed",
      text: "Password has been changed",
    });
    newPassword.value = undefined;
    console.log("Password Changed");
  })
    .catch((error) => {
      const resolved = resolveUserOperationError(
        error,
        "Unable to change password",
        "Review the password requirements and try again.",
      );
      console.error(resolved.debugMessage);
      showError(resolved.title, resolved.message);
    });
}

async function setActive(active: boolean) {
  if (statusUpdating.value || props.user.active === active) {
    return;
  }
  statusUpdating.value = true;
  try {
    await http.put(`/api/user-management/update/${props.user.id}/status`, {
      active,
    });
    notify({
      type: "success",
      title: active ? "User reactivated" : "User deactivated",
    });
    emit("refresh");
  } catch (error: any) {
    console.error(error);
    const resolved = resolveUserOperationError(
      error,
      "Unable to update status",
      "Failed to update user status.",
    );
    notify({
      type: "error",
      title: resolved.title,
      text: resolved.message,
    });
    showError(resolved.title, resolved.message);
  } finally {
    statusUpdating.value = false;
  }
}

async function deleteUser() {
  if (deletingUser.value) {
    return;
  }
  if (
    !window.confirm(
      `Are you sure you want to delete user "${props.user.username}"? This action cannot be undone.`,
    )
  ) {
    return;
  }
  deletingUser.value = true;
  try {
    await http.delete(`/api/user-management/delete/${props.user.id}`);
    notify({
      type: "success",
      title: "User deleted",
    });
    emit("deleted");
  } catch (error: any) {
    console.error(error);
    const resolved = resolveUserOperationError(
      error,
      "Unable to delete user",
      "Failed to delete user.",
    );
    notify({
      type: "error",
      title: resolved.title,
      text: resolved.message,
    });
    showError(resolved.title, resolved.message);
  } finally {
    deletingUser.value = false;
  }
}

function resolveUserOperationError(
  error: unknown,
  fallbackTitle: string,
  fallbackMessage: string,
): { title: string; message: string; debugMessage: string } {
  const fallback = {
    title: fallbackTitle,
    message: fallbackMessage,
    debugMessage: typeof error === "string" ? error : JSON.stringify(error),
  };

  if (isAxiosError(error)) {
    const status = error.response?.status;
    const data = error.response?.data;
    const payloadMessage =
      (typeof data === "string" && data.trim().length > 0 && data.trim()) ||
      (typeof data === "object" &&
        data !== null &&
        "message" in data &&
        typeof (data as { message?: unknown }).message === "string" &&
        (data as { message: string }).message.trim().length > 0
        ? (data as { message: string }).message.trim()
        : undefined);

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
@import "@/assets/styles/theme";
.admin-user-page {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}
.tabs {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 90vh;

  background-color: $background-30;
}
@media screen and (max-width: 800px) {
  .tabs-header {
    flex-direction: column;
  }
}
.tabs-header {
  display: flex;
  gap: 1rem;
  width: 100%;
  background-color: $primary-30;
}
.tab {
  padding: 1rem;
  cursor: pointer;
  border-radius: 0.5rem 0.5rem 0 0;
  border: 1px solid $primary-50;
  &:hover {
    background-color: $accent;
    color: white;
  }
}
.tab[data-active="true"] {
  background-color: $accent;
  color: white;
  cursor: default;
}
.tab-content {
  display: flex;
  width: 100%;
  height: 100%;
  margin: auto 0;
  border: 1px solid $primary-50;
  padding: 1rem;
}
.tab-content[data-active="false"] {
  display: none;
}
.tabs-content[data-active="true"] {
  display: block;
}
.config {
  width: 100%;
  height: 100%;
  margin: auto 0;
}
#userMain {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}
.userStatus {
  display: flex;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}
.statusBadge {
  padding: 0.35rem 0.75rem;
  border-radius: 999px;
  font-weight: 600;
  background-color: $primary-30;
  color: $text;
  &[data-active="true"] {
    background-color: $primary-70;
    color: $background;
  }
  &[data-active="false"] {
    background-color: $secondary-70;
    color: $text;
  }
}
.statusActions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.75rem;
}
.secondaryButton,
.dangerButton {
  border: none;
  border-radius: 0.5rem;
  padding: 0.5rem 1rem;
  font-weight: bold;
  cursor: pointer;
}
.secondaryButton {
  background-color: $primary-70;
  color: $text;
  &:hover {
    background-color: $primary-90;
  }
  &:disabled {
    background-color: $primary-30;
    cursor: not-allowed;
  }
}
.dangerButton {
  background-color: $accent;
  color: $background;
  &:hover {
    background-color: $accent-70;
  }
  &:disabled {
    background-color: $accent-30;
    cursor: not-allowed;
  }
}

form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
</style>
