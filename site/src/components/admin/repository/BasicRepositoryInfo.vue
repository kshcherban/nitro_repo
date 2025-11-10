<template>
  <v-card
    v-if="repository"
    data-testid="repository-info-card"
    class="repository-info-card">
    <v-card-title class="repository-info-card__header">
      <div>
        <div class="text-h6">Repository Info</div>
        <div class="text-body-2 text-medium-emphasis">
          Operational details and current usage metrics.
        </div>
      </div>
      <v-chip
        size="small"
        class="text-uppercase font-weight-medium"
        :color="statusChip.color"
        variant="tonal"
        data-testid="repository-status-chip">
        {{ statusChip.label }}
      </v-chip>
    </v-card-title>

    <v-card-text>
      <v-row
        class="repository-info-card__grid"
        dense
        data-testid="repository-meta-grid">
        <v-col
          v-for="item in metaItems"
          :key="item.label"
          cols="12"
          md="6"
          lg="4">
          <div
            class="meta-tile"
            data-testid="repository-meta-item">
            <span class="meta-tile__label">{{ item.label }}</span>
            <span class="meta-tile__value">{{ item.value }}</span>
          </div>
        </v-col>
      </v-row>

      <v-divider class="my-6" />

      <div class="repository-info-card__actions">
        <div class="repository-info-card__auth">
          <span class="text-subtitle-2">Repository Authentication</span>
          <span class="text-body-2 text-medium-emphasis">
            {{ repository.auth_enabled ? "Enabled" : "Disabled" }}
          </span>
        </div>
        <div class="repository-info-card__buttons">
          <v-btn
            :color="toggleButton.color"
            variant="tonal"
            class="text-none"
            data-testid="repository-toggle"
            @click="notify('This feature is not implemented yet')">
            <v-icon
              class="mr-2"
              icon="mdi-toggle-switch" />
            {{ toggleButton.label }}
          </v-btn>
          <v-btn
            color="error"
            variant="flat"
            class="text-none"
            data-testid="repository-delete"
            @click="deleteRepository">
            <v-icon
              class="mr-2"
              icon="mdi-delete-outline" />
            Delete Repository
          </v-btn>
        </div>
      </div>
    </v-card-text>
  </v-card>
</template>
<script setup lang="ts">
import http from "@/http";
import router from "@/router";
import type { RepositoryWithStorageName } from "@/types/repository";
import { notify } from "@kyvg/vue3-notification";
import { computed, type PropType } from "vue";

const props = defineProps({
  repository: {
    type: Object as PropType<RepositoryWithStorageName>,
    required: true,
  },
});

const statusChip = computed(() => {
  if (!props.repository) {
    return { label: "Unavailable", color: "warning" as const };
  }
  return props.repository.active
    ? { label: "Active", color: "success" as const }
    : { label: "Inactive", color: "warning" as const };
});

const toggleButton = computed(() => {
  if (!props.repository || props.repository.active) {
    return { label: "Disable Repository", color: "warning" as const };
  }
  return { label: "Enable Repository", color: "primary" as const };
});

const metaItems = computed(() => {
  if (!props.repository) {
    return [];
  }
  return [
    {
      label: "Repository Name",
      value: props.repository.name,
    },
    {
      label: "Repository Type",
      value: props.repository.repository_type,
    },
    {
      label: "Storage Name",
      value: props.repository.storage_name,
    },
    {
      label: "Storage Identifier",
      value: props.repository.storage_id,
    },
    {
      label: "Storage Usage",
      value: formatBytes(props.repository.storage_usage_bytes),
    },
    {
      label: "Usage Updated",
      value: formatUpdatedAt(props.repository.storage_usage_updated_at),
    },
  ];
});

function formatBytes(bytes?: number | null): string {
  if (bytes === null || bytes === undefined) {
    return "Unknown";
  }
  if (bytes === 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, exponent);
  return `${value.toFixed(exponent === 0 ? 0 : 2)} ${units[exponent]}`;
}

function formatUpdatedAt(timestamp?: string | null): string {
  if (!timestamp) {
    return "Unknown";
  }
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return "Unknown";
  }
  return date.toLocaleString();
}
async function deleteRepository() {
  http.delete(`/api/repository/${props.repository.id}`).then(() => {
    notify({
      type: "success",
      title: "Deleted",
      text: "Repository Deleted",
    });
    router.push({ name: "RepositoriesList" });
  });
}
</script>
<style lang="scss" scoped>
@use "@/assets/styles/theme.scss" as *;

.repository-info-card {
  &__header {
    align-items: flex-start;
    gap: 1rem;
  }

  &__grid {
    row-gap: 1rem;
  }

  &__actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 1.5rem;
  }

  &__auth {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  &__buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 0.75rem;
    justify-content: flex-end;
  }
}

.meta-tile {
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  padding: 0.75rem 1rem;
  background-color: rgba($primary, 0.07);
  border-radius: 12px;

  &__label {
    font-size: 0.8125rem;
    letter-spacing: 0.02em;
    font-weight: 600;
    color: $text-50;
    text-transform: uppercase;
  }

  &__value {
    font-size: 1rem;
    color: $text;
    word-break: break-word;
  }
}

@media (max-width: 960px) {
  .repository-info-card__header {
    flex-direction: column;
    align-items: flex-start;
  }
}
</style>
