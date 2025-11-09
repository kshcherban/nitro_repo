<template>
  <main>
    <FloatingErrorBanner
      :visible="errorBanner.visible"
      :title="errorBanner.title"
      :message="errorBanner.message"
      @close="resetError" />
    <h1>Repository Create</h1>
    <div v-if="currentRepositoryType">
      <h2>{{ currentRepositoryType.description }}</h2>
    </div>

    <form
      @submit.prevent="createRepository()"
      :class="{ 'go-repository-form': selectedRepositoryType === 'go' }">
      <TwoByFormBox>
        <TextInput
          id="repositoryName"
          v-model="input.name"
          autocomplete="none"
          required
          placeholder="Repository Name"
          >Repository Name</TextInput
        >
        <DropDown
          id="repositoryType"
          v-model="selectedRepositoryType"
          :options="repositoryTypeOptions"
          required
          class="form-field--medium"
          >Repository Type</DropDown
        >
        <DropDown
          id="storage"
          v-model="input.storage"
          :options="storageItemOptions"
          required
          class="form-field--medium"
          >Storage</DropDown
        >
      </TwoByFormBox>
      <div
        v-for="config in requiredConfigComponents"
        :key="config.component.name">
        <component
          :is="config.component"
          v-bind="config.props"
          v-model="requiredConfigValues[config.configName]" />
      </div>

      <div class="form-actions">
        <SubmitButton class="primary-action">Create</SubmitButton>
      </div>
    </form>
  </main>
</template>

<script lang="ts" setup>
import FallBackEditor from "@/components/admin/repository/configs/FallBackEditor.vue";
import DropDown from "@/components/form/dropdown/DropDown.vue";
import SubmitButton from "@/components/form/SubmitButton.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import type { StorageItem } from "@/components/nr/storage/storageTypes";
import FloatingErrorBanner from "@/components/ui/FloatingErrorBanner.vue";
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
    console.log(
      `Changed repository type to ${newValue} from '${old}'. Resetting required configs`,
    );
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
watch(requiredConfigValues, () => {
  console.log(requiredConfigValues.value);
});
const requiredConfigComponents = computed(() => {
  if (!currentRepositoryType.value) {
    return [];
  }

  const configs = currentRepositoryType.value?.required_configs.map((config) => {
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
  console.log(configs);
  return configs;
});

async function load() {
  await repoTypesStore.getStorages(true).then((response) => {
    storages.value = response;
  });

  await repoTypesStore.getRepositoryTypes().then((response) => {
    repositoryTypes.value = response;
  });
}

load();

async function createRepository() {
  const request = {
    name: input.value.name,
    storage: input.value.storage,
    configs: {} as any,
  };
  for (const [key, value] of Object.entries(requiredConfigValues.value)) {
    console.log(`${key} = ${JSON.stringify(value)}`);
    request.configs[key] = value;
  }
  resetError();
  console.log(JSON.stringify(request));
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
@import "@/assets/styles/theme.scss";

main {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
  position: relative;
  align-items: flex-start;
  width: 100%;
}

form {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
  width: 100%;
  max-width: 960px;
  padding: 1.5rem 0;

  &.go-repository-form {
    width: 100%;
    max-width: none;
    padding: 1.5rem 0;
  }
}

.form-actions {
  display: flex;
  justify-content: flex-start;
}

:deep(.primary-action) {
  width: auto;
  min-width: 160px;
  padding-inline: 1.75rem;
  align-self: flex-start;
}

:deep(.form-field--medium) {
  max-width: 320px;
  width: 100%;
}

:deep(.form-field--medium select) {
  width: 100%;
}

@media screen and (max-width: 1200px) {
  form {
    width: 100%;
  }
}
</style>
