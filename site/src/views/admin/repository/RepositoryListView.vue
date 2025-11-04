<template>
  <main>
    <section
      v-if="!error"
      class="usageToolbar">
      <div class="status">
        <strong>Storage usage cache:</strong>
        <span>{{ usageStatusText }}</span>
      </div>
      <button
        class="refreshButton"
        type="button"
        :disabled="refreshing || loading"
        @click="refreshUsage">
        {{ refreshing ? "Refreshing…" : "Refresh storage usage" }}
      </button>
    </section>
    <p v-if="loading && !error" class="infoText">Loading repositories…</p>
    <p v-else-if="error" class="errorText">{{ error }}</p>
    <div v-else-if="repositories.length >= 1">
      <RepositoryListInner :repositories="repositories" />
    </div>
    <p v-else class="infoText">No repositories found.</p>
  </main>
</template>

<script setup lang="ts">
import RepositoryListInner from "@/components/admin/repository/RepositoryListInner.vue";
import http from "@/http";
import { computed, ref } from "vue";

import type { RepositoryWithStorageName } from "@/types/repository";

const repositories = ref<RepositoryWithStorageName[]>([]);
const loading = ref(true);
const refreshing = ref(false);
const error = ref<string | null>(null);

async function fetchRepositories(options: { refresh?: boolean } = {}) {
  if (options.refresh) {
    refreshing.value = true;
  } else {
    loading.value = true;
  }
  error.value = null;
  try {
    const params: Record<string, boolean> = { include_usage: true };
    if (options.refresh) {
      params.refresh_usage = true;
    }
    const response = await http.get<RepositoryWithStorageName[]>("/api/repository/list", {
      params,
    });
    repositories.value = response.data;
  } catch (err) {
    console.error(err);
    error.value = "Failed to fetch repositories";
  } finally {
    if (options.refresh) {
      refreshing.value = false;
    } else {
      loading.value = false;
    }
  }
}

function refreshUsage() {
  void fetchRepositories({ refresh: true });
}

const latestUsageUpdate = computed(() => {
  const timestamps = repositories.value
    .map((repo) => repo.storage_usage_updated_at)
    .filter((value): value is string => Boolean(value));
  if (timestamps.length === 0) {
    return null;
  }
  const latest = timestamps.reduce((max, current) => (current > max ? current : max));
  return latest;
});

const usageStatusText = computed(() => {
  if (repositories.value.length === 0) {
    return "No repositories yet.";
  }
  if (!latestUsageUpdate.value) {
    return "Not calculated yet.";
  }
  const date = new Date(latestUsageUpdate.value);
  if (Number.isNaN(date.getTime())) {
    return "Not calculated yet.";
  }
  return `Last updated ${date.toLocaleString()}`;
});

void fetchRepositories();
</script>

<style scoped lang="scss">
@import "@/assets/styles/theme.scss";

.usageToolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  margin-bottom: 1.5rem;
  padding: 1rem 1.25rem;
  border-radius: 0.75rem;
  background: $background-50;
  border: 1px solid $primary-50;

  .status {
    display: flex;
    flex-direction: column;
    font-size: 0.95rem;

    span {
      color: $text-50;
    }
  }
}

.refreshButton {
  border: none;
  border-radius: 0.6rem;
  padding: 0.6rem 1.2rem;
  font-weight: 600;
  cursor: pointer;
  background: $primary-70;
  color: $background;
  transition: background 0.2s ease-in-out;

  &:hover:not(:disabled) {
    background: $primary-90;
  }

  &:disabled {
    cursor: not-allowed;
    background: $primary-30;
    color: $text-50;
  }
}

.infoText,
.errorText {
  margin: 0;
  padding: 1rem 0;
}

.errorText {
  color: $accent;
}
</style>
