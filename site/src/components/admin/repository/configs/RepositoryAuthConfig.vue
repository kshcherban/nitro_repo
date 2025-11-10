<template>
  <v-card
    class="auth-config"
    data-testid="auth-config-card">
    <v-card-text class="auth-config__content">
      <SwitchInput
        v-model="enabled"
        id="repository-auth-toggle"
        :disabled="isSaving">
        Require authentication for repository access
        <template #comment>
          Clients must authenticate using a Nitro Repo user/password or token before accessing this repository.
        </template>
      </SwitchInput>

      <v-alert
        v-if="!isCreate"
        density="comfortable"
        :type="alertState.type"
        variant="tonal"
        class="auth-config__status">
        {{ alertState.message }}
      </v-alert>
    </v-card-text>
  </v-card>
</template>

<script setup lang="ts">
import http from "@/http";
import SwitchInput from "@/components/form/SwitchInput.vue";
import { computed, onMounted, ref, watch } from "vue";

const props = defineProps<{
  repository?: string;
}>();

const model = defineModel<{ enabled: boolean }>({
  default: { enabled: false },
});

const isCreate = computed(() => !props.repository);
const isSaving = ref(false);
const error = ref<string | null>(null);
const hasLoaded = ref(false);
const enabled = computed({
  get: () => model.value.enabled,
  set: (value: boolean) => {
    model.value = { ...model.value, enabled: value };
  },
});

const alertState = computed(() => {
  if (error.value) {
    return { type: "error" as const, message: `Failed to save: ${error.value}` };
  }
  if (isSaving.value) {
    return { type: "info" as const, message: "Saving…" };
  }
  if (hasLoaded.value) {
    return { type: "success" as const, message: "Authentication settings saved." };
  }
  return { type: "info" as const, message: "Loading…" };
});

onMounted(load);

watch(
  () => enabled.value,
  async (enabled) => {
    if (!props.repository || !hasLoaded.value) {
      return;
    }
    error.value = null;
    isSaving.value = true;
    try {
      await http.put(`/api/repository/${props.repository}/config/auth`, {
        enabled,
      });
    } catch (err: any) {
      console.error(err);
      error.value =
        err?.response?.data?.message ?? err?.message ?? "Unknown error";
    } finally {
      isSaving.value = false;
    }
  },
);

async function load() {
  if (!props.repository) {
    hasLoaded.value = true;
    return;
  }
  try {
    const response = await http.get(
      `/api/repository/${props.repository}/config/auth`,
      {
        params: { default: true },
      },
    );
    if (response?.data) {
      model.value = response.data;
    }
  } catch (err) {
    console.error(err);
    error.value = "Failed to load configuration";
  } finally {
    hasLoaded.value = true;
  }
}
</script>

<style scoped lang="scss">
@use "@/assets/styles/theme.scss" as *;

.auth-config__content {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.auth-config__status {
  margin-top: 0.5rem;
}
</style>
