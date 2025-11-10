<template>
  <form class="helm-config" @submit.prevent="save">
    <div class="helm-config__grid" v-if="value">
      <DropDown
        v-model="value.mode"
        :options="modeOptions"
        required
        id="helm-mode"
      >Repository Mode</DropDown>

      <SwitchInput v-model="value.overwrite" id="helm-allow-overwrite">
        Allow Overwrite
      </SwitchInput>

      <TextInput
        v-model="publicBaseUrl"
        placeholder="https://charts.example.com/myrepo"
        id="helm-public-base-url"
      >Public Base URL</TextInput>

      <TextInput
        v-model="indexCacheTtl"
        inputmode="numeric"
        pattern="[0-9]*"
        placeholder="300"
        id="helm-index-ttl"
      >Index Cache TTL (seconds)</TextInput>

      <TextInput
        v-model="maxChartSize"
        inputmode="numeric"
        pattern="[0-9]*"
        placeholder="10485760"
        id="helm-max-chart-size"
      >Max Chart Size (bytes)</TextInput>

      <TextInput
        v-model="maxFileCount"
        inputmode="numeric"
        pattern="[0-9]*"
        placeholder="1024"
        id="helm-max-file-count"
      >Max Files Per Chart</TextInput>
    </div>

    <p class="helm-config__hint">
      Hybrid mode exposes classic HTTP chart downloads and OCI registry endpoints simultaneously.
    </p>

    <div
      v-if="!isCreate"
      class="helm-config__actions"
    >
      <button
        class="nr-button nr-button--primary"
        type="submit"
      >Save</button>
    </div>
  </form>
</template>
<script setup lang="ts">
import { computed, defineProps, onMounted } from "vue";
import DropDown from "@/components/form/dropdown/DropDown.vue";
import SwitchInput from "@/components/form/SwitchInput.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import http from "@/http";
import {
  defaultHelmConfig,
  type HelmRepositoryConfig,
  helmModeOptions,
} from "./helm";

const props = defineProps({
  settingName: String,
  repository: {
    type: String,
    required: false,
  },
});

const value = defineModel<HelmRepositoryConfig>({
  default: defaultHelmConfig(),
});

const isCreate = computed(() => !props.repository);

const modeOptions = helmModeOptions;

const publicBaseUrl = computed({
  get: () => value.value?.public_base_url ?? "",
  set: (input: string) => {
    if (!value.value) return;
    const trimmed = input.trim();
    value.value.public_base_url = trimmed === "" ? undefined : trimmed;
  },
});

function parseOptionalNumber(input: string): number | undefined {
  const trimmed = input.trim();
  if (trimmed === "") {
    return undefined;
  }
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : undefined;
}

const indexCacheTtl = computed({
  get: () => value.value?.index_cache_ttl?.toString() ?? "",
  set: (input: string) => {
    if (!value.value) return;
    value.value.index_cache_ttl = parseOptionalNumber(input);
  },
});

const maxChartSize = computed({
  get: () => value.value?.max_chart_size?.toString() ?? "",
  set: (input: string) => {
    if (!value.value) return;
    value.value.max_chart_size = parseOptionalNumber(input);
  },
});

const maxFileCount = computed({
  get: () => value.value?.max_file_count?.toString() ?? "",
  set: (input: string) => {
    if (!value.value) return;
    value.value.max_file_count = parseOptionalNumber(input);
  },
});

async function load() {
  if (!props.repository) {
    value.value = defaultHelmConfig();
    return;
  }
  try {
    const response = await http.get(`/api/repository/${props.repository}/config/helm`);
    value.value = response.data;
  } catch (error) {
    console.error("Failed to load Helm config", error);
  }
}

async function save() {
  if (!props.repository || !value.value) {
    return;
  }
  try {
    await http.put(`/api/repository/${props.repository}/config/helm`, value.value);
    await load();
  } catch (error) {
    console.error("Failed to save Helm config", error);
  }
}

onMounted(() => {
  if (!value.value) {
    value.value = defaultHelmConfig();
  }
  load();
});
</script>

<style scoped lang="scss">
@import "@/assets/styles/theme.scss";
@import "@/assets/styles/buttons.scss";

.helm-config {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.helm-config__grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 1rem;
}

.helm-config__hint {
  margin: 0;
  font-size: 0.9rem;
  color: $text-50;
}

.helm-config__actions {
  display: flex;
  justify-content: flex-end;
}

.helm-config__actions .nr-button {
  min-width: 8rem;
}
</style>
