<template>
  <section class="packages">
    <v-card>
      <v-card-title class="d-flex align-center pa-4">
        <span class="text-h6">{{ headerTitle }}</span>
        <v-spacer />
        <v-text-field
          v-if="!isLoading && totalPackages > 0"
          v-model="searchTerm"
          :placeholder="`Search ${headerTitle.toLowerCase()}…`"
          prepend-inner-icon="mdi-magnify"
          variant="outlined"
          density="compact"
          clearable
          @click:clear="clearSearch"
          hide-details
          style="max-width: 300px;"
          aria-label="Search packages" />
      </v-card-title>

      <v-card-subtitle v-if="!isLoading" class="pa-4 pt-0">
        <div class="d-flex align-center justify-space-between flex-wrap">
          <div class="text-body-2 text-medium-emphasis">
            {{ totalPackages }} package(s)
            <span v-if="visiblePackages.length"> · Showing {{ visiblePackages.length }} file(s)</span>
          </div>
          <div class="d-flex align-center gap-3">
            <v-btn
              color="primary"
              variant="text"
              prepend-icon="mdi-refresh"
              :disabled="isLoading || isDeleting"
              @click="refreshPackages">
              Refresh
            </v-btn>
            <span v-if="selectedCount > 0" class="text-body-2">{{ selectedCount }} selected</span>
            <v-btn
              color="error"
              variant="flat"
              prepend-icon="mdi-delete"
              :disabled="selectedCount === 0 || isDeleting || isDockerProxy"
              :loading="isDeleting"
              @click="deleteSelected">
              Delete Selected
            </v-btn>
          </div>
        </div>
      </v-card-subtitle>

      <v-card-text
        v-if="!isLoading && pendingDeletionCount > 0"
        class="pt-0 px-4">
        <v-alert
          type="info"
          variant="tonal"
          border="start"
          class="packages__deletion-alert">
          <div class="text-body-2">
            {{ pendingDeletionCount }} package{{ pendingDeletionCount === 1 ? "" : "s" }} queued for deletion. Changes may take a moment to complete. Use Refresh to check for updates.
          </div>
        </v-alert>
      </v-card-text>

      <v-data-table
        v-if="!isLoading && !error && totalPackages > 0 && visiblePackages.length > 0"
        :headers="headers"
        :items="tableItems"
        :search="searchTerm"
        :loading="isDeleting"
        item-value="cachePath"
        v-model="selected"
        show-select
        class="elevation-0"
        :items-per-page="perPage"
        :item-length="totalPackages">

        <template v-slot:item.size="{ value }">
          <div class="text-end">{{ formatBytes(value) }}</div>
        </template>

        <template v-slot:item.cachePath="{ value }">
          <v-code class="text-caption">{{ value }}</v-code>
        </template>

        <template v-slot:item.modified="{ value }">
          <div class="text-no-wrap">
            {{ new Date(value).toLocaleString() }}
          </div>
        </template>

        <template v-slot:no-data>
          <div class="pa-4 text-center text-medium-emphasis">
            No packages match your search. Try different search terms.
          </div>
        </template>
      </v-data-table>

      <v-card-text v-else-if="isLoading" class="text-center py-8">
        <v-progress-circular indeterminate color="primary" size="48" />
        <div class="mt-4 text-medium-emphasis">Loading packages...</div>
      </v-card-text>

      <v-card-text v-else-if="error" class="text-center py-8">
        <v-icon color="error" size="48" class="mb-2">mdi-alert-circle</v-icon>
        <div class="text-error">Failed to load packages: {{ error }}</div>
      </v-card-text>

      <v-card-text v-else-if="totalPackages === 0" class="text-center py-8">
        <v-icon color="medium-emphasis" size="48" class="mb-2">mdi-package-variant</v-icon>
        <div class="text-medium-emphasis">{{ emptyRepositoryMessage }}</div>
      </v-card-text>

      <v-card-text v-else-if="visiblePackages.length === 0" class="text-center py-8">
        <v-icon color="medium-emphasis" size="48" class="mb-2">mdi-magnify</v-icon>
        <div class="text-medium-emphasis">No packages match your search on this page. Try a different page or clear the filters.</div>
      </v-card-text>

      <v-card-actions v-if="totalPackages > 0" class="pa-4">
        <v-spacer />
        <v-pagination
          v-model="currentPage"
          :length="totalPages"
          :disabled="isDeleting" />
        <v-spacer />

      </v-card-actions>
    </v-card>
  </section>
</template>

<script setup lang="ts">
import http from "@/http";
import { computed, onMounted, ref, watch } from "vue";
import { useAlertsStore } from "@/stores/alerts";
import { useResizableColumns } from "@/composables/useResizableColumns";

interface PackageEntry {
  name: string;
  size: number;
  cachePath: string;
  modified: string;
  package: string;
}

const props = defineProps<{
  repositoryId: string;
  repositoryType?: string;
  repositoryKind?: string | null;
}>();

const packages = ref<PackageEntry[]>([]);
const isLoading = ref(false);
const error = ref<string | null>(null);
const currentPage = ref(1);
const perPage = ref(50);
const totalPackages = ref(0);
const perPageOptions = [25, 50, 100, 200];
const selected = ref<string[]>([]);
const isDeleting = ref(false);
const searchTerm = ref("");
const pendingDeletionPaths = ref<string[]>([]);
const pendingDeletionCount = ref(0);
const alerts = useAlertsStore();

function clearSearch() {
  searchTerm.value = "";
}

onMounted(() => {
  loadPackages();
  // Enable resizable columns for v-data-table
  useResizableColumns('.v-data-table th');
});
watch(
  () => props.repositoryId,
  () => {
  packages.value = [];
  error.value = null;
  currentPage.value = 1;
  selected.value = [];
  searchTerm.value = "";
  pendingDeletionPaths.value = [];
  pendingDeletionCount.value = 0;
  loadPackages();
  },
);

watch([currentPage, perPage], () => {
  if (!props.repositoryId) {
    return;
  }
  selected.value = [];
  loadPackages();
});

// Define table headers based on repository type
const headers = computed(() => [
  {
    title: packageColumnTitle.value,
    key: 'package',
    sortable: true,
  },
  {
    title: nameColumnTitle.value,
    key: 'name',
    sortable: true,
  },
  {
    title: 'Size',
    key: 'size',
    sortable: true,
    align: 'end' as const,
  },
  {
    title: pathColumnTitle.value,
    key: 'cachePath',
    sortable: true,
  },
  {
    title: timestampColumnTitle.value,
    key: 'modified',
    sortable: true,
  },
]);

// Convert packages to v-data-table format
const tableItems = computed(() => {
  return packages.value.map((pkg) => ({
    package: pkg.package,
    name: pkg.name,
    size: pkg.size,
    cachePath: pkg.cachePath,
    modified: pkg.modified,
  }));
});

const totalPages = computed(() => {
  if (totalPackages.value === 0) {
    return 1;
  }
  return Math.max(1, Math.ceil(totalPackages.value / perPage.value));
});

const normalizedSearchTerm = computed(() => searchTerm.value.trim().toLowerCase());

const visiblePackages = computed(() => {
  const term = normalizedSearchTerm.value;
  if (!term) {
    return packages.value;
  }
  return packages.value.filter((pkg) => {
    const haystack = [pkg.package, pkg.name, pkg.cachePath].join(" ").toLowerCase();
    return haystack.includes(term);
  });
});

const selectedCount = computed(() => selected.value.length);

const derivedHostedFromPackages = computed(() => {
  if (packages.value.length === 0) {
    return false;
  }
  return !packages.value.some((pkg) => pkg.cachePath.startsWith("packages/"));
});

const repositoryType = computed(() => props.repositoryType?.toLowerCase() ?? "");
const isDockerRepository = computed(() => repositoryType.value === "docker");
const isDebRepository = computed(() => repositoryType.value === "deb");
const isDockerProxy = computed(
  () => isDockerRepository.value && props.repositoryKind?.toLowerCase() === "proxy",
);

const isHostedRepository = computed(() => {
  if (props.repositoryKind) {
    return props.repositoryKind.toLowerCase() === "hosted";
  }
  if (isDockerRepository.value || isDebRepository.value) {
    return true;
  }
  if (props.repositoryType === "python") {
    return derivedHostedFromPackages.value;
  }
  return false;
});

const headerTitle = computed(() => {
  if (isDockerRepository.value) {
    return "Images";
  }
  return "Packages";
});
const packageColumnTitle = computed(() => (isDockerRepository.value ? "Repository" : "Package"));
const nameColumnTitle = computed(() => {
  if (isDockerRepository.value) {
    return "Tag";
  }
  if (isDebRepository.value) {
    return "Version";
  }
  return "Name";
});
const pathColumnTitle = computed(() => {
  if (isDockerRepository.value) {
    return "Manifest Path";
  }
  return isHostedRepository.value ? "Path" : "Cached Path";
});
const timestampColumnTitle = computed(() =>
  "Uploaded At",
);

const emptyRepositoryMessage = computed(() => {
  if (isDockerRepository.value) {
    if (isDockerProxy.value) {
      return "No images cached yet. Pull an image through this proxy to populate the list.";
    }
    return "No images yet. Push an image to populate this list.";
  }
  return isHostedRepository.value
    ? "No packages yet. Upload a package to populate this list."
    : "No packages yet. Trigger a download to populate this list.";
});

async function loadPackages() {
  if (!props.repositoryId) {
    return;
  }
  isLoading.value = true;
  error.value = null;
  try {
    const response = await http.get(`/api/repository/${props.repositoryId}/packages`, {
      params: { page: currentPage.value, per_page: perPage.value },
    });
    const data = response.data ?? {};
    const items: PackageEntry[] = (data.items ?? []).map((item: any) => ({
      name: item.name,
      size: item.size,
      cachePath: item.cache_path,
      modified: item.modified,
      package: item.package,
    }));
    packages.value = items;
    totalPackages.value = data.total_packages ?? 0;
    selected.value = [];

    if (pendingDeletionPaths.value.length > 0) {
      const remaining = pendingDeletionPaths.value.filter((path) =>
        items.some((pkg) => pkg.cachePath === path),
      );
      pendingDeletionPaths.value = remaining;
      pendingDeletionCount.value = remaining.length;
    } else {
      pendingDeletionCount.value = 0;
    }
  } catch (err) {
    console.error(err);
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    isLoading.value = false;
  }
}

async function deleteSelected() {
  if (!props.repositoryId || selected.value.length === 0) {
    return;
  }
  const paths = [...selected.value];
  const count = selected.value.length;
  const confirmationMessage = isDockerRepository.value
    ? `Delete ${count} image tag(s)? This removes their manifests from the registry.`
    : isHostedRepository.value
      ? `Delete ${count} package(s)? This removes files from the repository.`
      : `Delete ${count} cached package(s)? This removes cached files but not upstream artifacts.`;
  const confirmed = window.confirm(confirmationMessage);
  if (!confirmed) {
    return;
  }
  isDeleting.value = true;
  try {
    await http.delete(`/api/repository/${props.repositoryId}/packages`, {
      data: { paths: selected.value },
    });
    const successTitle = isDockerRepository.value ? "Images deleted" : "Packages deleted";
    const successText = isDockerRepository.value
      ? `${count} manifest(s) removed`
      : isHostedRepository.value
        ? `${count} package(s) removed`
        : `${count} cached package(s) removed`;
    alerts.success(successTitle, successText);
    pendingDeletionPaths.value = paths;
    pendingDeletionCount.value = paths.length;
    selected.value = [];
    await loadPackages();
  } catch (err: any) {
    console.error(err);
    const message = err?.response?.data?.message ?? err?.message ?? "Failed to delete packages";
    alerts.error("Deletion failed", message);
  } finally {
    isDeleting.value = false;
  }
}

async function refreshPackages() {
  if (isLoading.value || isDeleting.value) {
    return;
  }
  await loadPackages();
}


function formatBytes(bytes: number): string {
  if (bytes === 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  const idx = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, idx);
  return `${value.toFixed(idx === 0 ? 0 : 2)} ${units[idx]}`;
}

</script>

<style scoped lang="scss">
.packages {
  padding: 1rem;
}

// Ensure v-data-table respects theme colors and add animations
:deep(.v-data-table) {
  .v-data-table__th {
    color: var(--nr-text-primary);
    background-color: var(--nr-table-header-background);
    font-weight: 500;
    transition: all 0.2s ease;
  }

  .v-data-table__td {
    color: var(--nr-text-primary);
    transition: all 0.2s ease;
  }

  .v-data-table__tr {
    transition: all 0.2s ease;

    &:hover {
      background-color: var(--nr-table-row-hover);
      transform: scale(1.001);
    }
  }

  // Responsive improvements
  @media (max-width: 960px) {
    .v-data-table__th,
    .v-data-table__td {
      padding: 8px 12px;
      font-size: 0.875rem;
    }
  }

  @media (max-width: 600px) {
    .v-data-table__th,
    .v-data-table__td {
      padding: 6px 8px;
      font-size: 0.8rem;
    }
  }
}

// Add smooth card transitions
.v-card {
  transition: all 0.3s ease;
}

.packages__deletion-alert {
  margin: 0;
}
</style>
