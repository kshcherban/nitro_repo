<template>
  <section class="auth-config">
    <label class="toggle">
      <input type="checkbox" v-model="enabled" :disabled="isSaving" />
      <span>Require authentication for repository access</span>
    </label>
    <p class="hint">
      When enabled, clients must authenticate using a Nitro Repo user/password or token before
      accessing this repository.
    </p>
    <p v-if="!isCreate" class="status" :class="{ 'status--error': error }">
      <template v-if="error">Failed to save: {{ error }}</template>
      <template v-else-if="isSaving">Saving…</template>
      <template v-else-if="hasLoaded">Saved</template>
    </p>
 </section>
</template>

<script setup lang="ts">
import http from "@/http";
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
@import "@/assets/styles/theme";

.auth-config {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.toggle {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-weight: 600;
}

.hint {
  color: $secondary;
  font-size: 0.9rem;
  margin: 0;
}

.status {
  margin: 0;
  font-size: 0.85rem;
  color: $secondary;
}

.status--error {
  color: var(--error-color, #d9534f);
}
</style>
