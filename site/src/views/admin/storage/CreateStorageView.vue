<template>
  <main>
    <FloatingErrorBanner
      :visible="errorBanner.visible"
      :title="errorBanner.title"
      :message="errorBanner.message"
      @close="resetError" />
    <h1>Storage Create</h1>
    <form @submit.prevent="createStorage()">
      <TwoByFormBox>
        <TextInput
          id="storageName"
          v-model="input.name"
          autocomplete="none"
          required
          placeholder="Storage Name"
          >Storage Name</TextInput
        >
        <DropDown
          id="storageType"
          v-model="input.storageType"
          :options="storageOptions"
          required
          class="form-field--medium"
          >Storage Type</DropDown
        >
      </TwoByFormBox>
      <div
        v-if="storageConfig"
        class="storageConfig">
        <h2>{{ storageConfig.title }}</h2>
        <component
          :is="storageConfig.component"
          v-model="input.storageConfigValue"></component>
      </div>
      <SubmitButton
        v-if="storageConfig"
        class="primary-action">
        Create
      </SubmitButton>
    </form>
  </main>
</template>

<script lang="ts" setup>
import DropDown from "@/components/form/dropdown/DropDown.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import TwoByFormBox from "@/components/form/TwoByFormBox.vue";
import FloatingErrorBanner from "@/components/ui/FloatingErrorBanner.vue";
import { getStorageType, storageTypes } from "@/components/nr/storage/storageTypes";
import http from "@/http";
import router from "@/router";
import { notify } from "@kyvg/vue3-notification";
import { computed, ref, watch } from "vue";
import { isAxiosError } from "axios";
const input = ref({
  name: "",
  storageType: "",
  storageConfigValue: {},
});
const storageOptions = ref(storageTypes);
const storageConfig = computed(() => {
  if (input.value.storageType === "") {
    return undefined;
  }
  const current = getStorageType(input.value.storageType);
  return current;
});

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

watch(
  () => input.value.storageType,
  () => {
    resetError();
  },
);

async function createStorage() {
  console.log(input.value);
  const data = {
    name: input.value.name,
    config: {
      type: input.value.storageType,
      settings: input.value.storageConfigValue,
    },
  };

  resetError();
  await http
    .post(`/api/storage/new/${input.value.storageType}`, data)
    .then((response) => {
      console.log(response);
      notify({
        type: "success",
        title: "Storage Created",
        text: "The storage has been created.",
      });
      router.push({
        name: "ViewStorage",
        params: { id: response.data.id },
      });
    })
    .catch((err) => {
      const resolved = resolveStorageError(err);
      errorBanner.value.visible = true;
      errorBanner.value.title = resolved.title;
      errorBanner.value.message = resolved.message;
      console.error(resolved.debugMessage);
    });
}

function resolveStorageError(error: unknown): {
  title: string;
  message: string;
  debugMessage: string;
} {
  const fallback = {
    title: "Unable to create storage",
    message: "An unexpected error occurred. Please try again.",
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

    if (status === 409) {
      return {
        title: "Storage name already exists",
        message:
          payloadMessage ??
          "A storage with the same name already exists. Choose a different storage name.",
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
@use "@/assets/styles/tokens.scss" as *;
form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  width: 100%;
  max-width: 720px;
  padding: 1rem 0;
}
.storageConfig {
  padding: 1rem;
  border: 1px solid var(--nr-border-color);
  border-radius: 0.5rem;
}
@media screen and (max-width: 1200px) {
  form {
    width: 100%;
  }
}
main {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  align-items: flex-start;
}

:deep(.primary-action) {
  align-self: flex-start;
  width: auto;
  min-width: 160px;
}

:deep(.form-field--medium) {
  max-width: 320px;
  width: 100%;
}
</style>
