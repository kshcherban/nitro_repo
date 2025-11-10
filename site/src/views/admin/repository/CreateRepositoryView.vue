<template>
  <v-container class="py-6">
    <v-alert
      v-if="errorBanner.visible"
      type="error"
      variant="tonal"
      class="mb-4"
      closable
      @click:close="resetError">
      <div class="text-subtitle-1 font-weight-medium mb-1">{{ errorBanner.title }}</div>
      <div>{{ errorBanner.message }}</div>
    </v-alert>

    <v-card
      data-testid="repository-create-card"
      :class="{ 'go-repository-form': selectedRepositoryType === 'go' }">
      <v-card-title class="d-flex align-center justify-space-between">
        <div>
          <div class="text-h6">Create Repository</div>
          <div class="text-body-2 text-medium-emphasis" v-if="currentRepositoryType">
            {{ currentRepositoryType.description }}
          </div>
        </div>
      </v-card-title>

      <v-card-text>
        <v-form @submit.prevent="createRepository()">
          <v-row dense>
            <v-col cols="12" md="6">
              <TextInput
                id="repositoryName"
                v-model="input.name"
                autocomplete="off"
                required
                placeholder="Repository Name">
                Repository Name
              </TextInput>
            </v-col>
            <v-col cols="12" md="6">
              <DropDown
                id="repositoryType"
                v-model="selectedRepositoryType"
                :options="repositoryTypeOptions"
                required>
                Repository Type
              </DropDown>
            </v-col>
            <v-col cols="12" md="6">
              <DropDown
                id="storage"
                v-model="input.storage"
                :options="storageItemOptions"
                required>
                Storage
              </DropDown>
            </v-col>
          </v-row>

          <div
            v-for="config in requiredConfigComponents"
            :key="config.configName"
            class="mt-6">
            <component
              :is="config.component"
              v-bind="config.props"
              v-model="requiredConfigValues[config.configName]" />
          </div>

          <div class="d-flex justify-end mt-6">
            <SubmitButton
              color="primary"
              :loading="isSubmitting"
              :disabled="isSubmitting">
              <span v-if="isSubmitting">Creating…</span>
              <span v-else>Create</span>
            </SubmitButton>
          </div>
        </v-form>
      </v-card-text>
    </v-card>
  </v-container>
</template>

<script lang="ts" setup>
import FallBackEditor from "@/components/admin/repository/configs/FallBackEditor.vue";
import DropDown from "@/components/form/dropdown/DropDown.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import type { StorageItem } from "@/components/nr/storage/storageTypes";
import http from "@/http";
import router from "@/router";
import { useRepositoryStore } from "@/stores/repositories";
import { getConfigType, getConfigTypeDefault, type RepositoryTypeDescription } from "@/types/repository";
import { notify } from "@kyvg/vue3-notification";
import { computed, ref, watch } from "vue";
import { isAxiosError } from "axios";
const input = ref({
  name: "",
  storage: "",
});
const repoTypesStore = useRepositoryStore();
const selectedRepositoryType = ref("");
const repositoryTypes = ref<RepositoryTypeDescription[]>([]);
const storages = ref<StorageItem[]>([]);
const storageItemOptions = computed(() => {
  return storages.value.map((storage) => {
    return {
      value: storage.id,
      label: `${storage.name} (${storage.storage_type})`,
    };
  });
});
const repositoryTypeOptions = computed(() => {
  return repositoryTypes.value.map((type) => {
    return {
      value: type.type_name,
      label: type.name,
    };
  });
});
const currentRepositoryType = computed(() => {
  return repositoryTypes.value.find((type) => type.type_name === selectedRepositoryType.value);
});
const requiredConfigValues = ref<Record<string, any>>({});
const errorBanner = ref({
  visible: false,
  title: "",
  message: "",
});
const isSubmitting = ref(false);
const resetError = () => {
  errorBanner.value.visible = false;
  errorBanner.value.title = "";
  errorBanner.value.message = "";
};
watch(
  selectedRepositoryType,
  async (newValue, old) => {
    if (newValue === old) {
      return;
    }
    resetError();
    requiredConfigValues.value = {} as Record<string, any>;
    for (const config of currentRepositoryType.value?.required_configs || []) {
      try {
        const defaultValue = await getConfigTypeDefault(config);
        requiredConfigValues.value[config] = defaultValue ?? {};
      } catch (error) {
        console.error(`Failed to load default config for ${config}`, error);
        requiredConfigValues.value[config] = {};
      }
    }
  },
);
const requiredConfigComponents = computed(() => {
  if (!currentRepositoryType.value) {
    return [];
  }

  return currentRepositoryType.value.required_configs.map((config) => {
    const component = getConfigType(config);
    if (component) {
      return {
        component: component.component,
        configName: config,
      };
    } else {
      return {
        component: FallBackEditor,
        configName: config,
        props: {
          settingName: config,
        },
      };
    }
  });
});

async function load() {
  await repoTypesStore.getStorages(true).then((response) => {
    storages.value = response;
  });

  await repoTypesStore.getRepositoryTypes().then((response) => {
    repositoryTypes.value = response;
  });
}

void load();

async function createRepository() {
  const request = {
    name: input.value.name,
    storage: input.value.storage,
    configs: {} as any,
  };
  for (const [key, value] of Object.entries(requiredConfigValues.value)) {
    request.configs[key] = value;
  }
  resetError();
  isSubmitting.value = true;
  await http
    .post(`/api/repository/new/${selectedRepositoryType.value}`, request)
    .then((response) => {
      notify({
        type: "success",
        title: "Success",
        text: "Repository created",
      });
      router.push({
        name: "AdminViewRepository",
        params: { id: response.data.id },
      });
    })
    .catch((error) => {
      const resolved = resolveRepositoryError(error);
      errorBanner.value.visible = true;
      errorBanner.value.title = resolved.title;
      errorBanner.value.message = resolved.message;
      console.error(resolved.debugMessage);
    })
    .finally(() => {
      isSubmitting.value = false;
    });
}

function resolveRepositoryError(error: unknown): {
  title: string;
  message: string;
  debugMessage: string;
} {
  const fallback = {
    title: "Unable to create repository",
    message: "An unexpected error occurred. Please try again.",
    debugMessage: typeof error === "string" ? error : JSON.stringify(error),
  };

  if (isAxiosError(error)) {
    const status = error.response?.status;
    const data = error.response?.data;
    let payloadMessage: string | undefined;
    if (typeof data === "string") {
      const trimmed = data.trim();
      if (trimmed.length > 0) {
        payloadMessage = trimmed;
      }
    } else if (typeof data === "object" && data !== null && "message" in data) {
      const candidate = (data as { message?: unknown }).message;
      if (typeof candidate === "string" && candidate.trim().length > 0) {
        payloadMessage = candidate.trim();
      }
    }

    if (status === 409) {
      return {
        title: "Repository name already exists",
        message:
          payloadMessage ??
          "A repository with the same name already exists on this storage. Choose a different name.",
        debugMessage: JSON.stringify(error.toJSON?.() ?? error),
      };
    }

    if (payloadMessage) {
      return {
        title: fallback.title,
        message: payloadMessage as string,
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
.go-repository-form {
  :deep(.v-row) {
    gap: 1.5rem 0;
  }
}
</style>
