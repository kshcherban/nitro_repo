<template>
  <section class="packages">
    <header class="packages__header">
      <div class="packages__title-row">
        <h2>{{ headerTitle }}</h2>
        <div class="packages__search" v-if="!isLoading && totalPackages > 0">
          <input
            type="search"
            :placeholder="`Search ${headerTitle.toLowerCase()}…`"
            v-model="searchTerm"
            aria-label="Search packages" />
        </div>
      </div>
      <div v-if="!isLoading" class="packages__header-meta">
        <div class="packages__counts">
          <span>{{ totalPackages }} package(s)</span>
          <span v-if="visiblePackages.length">
            Showing {{ visiblePackages.length }} file(s)
          </span>
        </div>
        <div class="packages__actions">
          <span v-if="selectedCount > 0">{{ selectedCount }} selected</span>
          <button
            class="nr-button nr-button--danger"
            type="button"
            @click="deleteSelected"
            :disabled="selectedCount === 0 || isDeleting">
            Delete Selected
          </button>
        </div>
      </div>
    </header>

    <div v-if="isLoading" class="packages__state">Loading packages...</div>
    <div v-else-if="error" class="packages__state packages__state--error">
      Failed to load packages: {{ error }}
    </div>
    <div v-else-if="totalPackages === 0" class="packages__state">
      {{ emptyRepositoryMessage }}
    </div>
    <div v-else-if="visiblePackages.length === 0" class="packages__state">
      No packages match your search on this page. Try a different page or clear the filters.
    </div>
    <table v-else class="packages__table">
      <thead>
        <tr>
          <th class="packages__checkbox">
            <input
              type="checkbox"
              :checked="allSelected"
              :indeterminate.prop="isIndeterminate"
              @change="toggleSelectAll"
              :disabled="isDeleting" />
          </th>
          <th>Package</th>
          <th>Name</th>
          <th>Size</th>
          <th>{{ pathColumnTitle }}</th>
          <th>Cached At</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="pkg in visiblePackages" :key="pkg.cachePath">
          <td class="packages__checkbox">
            <input
              type="checkbox"
              :value="pkg.cachePath"
              v-model="selected"
              :disabled="isDeleting" />
          </td>
          <td>{{ pkg.package }}</td>
          <td>{{ pkg.name }}</td>
          <td>{{ formatBytes(pkg.size) }}</td>
          <td><code>{{ pkg.cachePath }}</code></td>
          <td>{{ new Date(pkg.modified).toLocaleString() }}</td>
        </tr>
      </tbody>
    </table>
    <div v-if="totalPackages > 0" class="packages__pager">
      <button class="nr-button" type="button" @click="prevPage" :disabled="currentPage === 1">
        Previous
      </button>
      <span>{{ pageLabel }}</span>
      <button
        class="nr-button"
        type="button"
        @click="nextPage"
        :disabled="currentPage >= totalPages">
        Next
      </button>
      <label class="packages__pager-select">
        Per page
        <select :value="perPage" @change="updatePerPage">
          <option v-for="option in perPageOptions" :key="option" :value="option">
            {{ option }}
          </option>
        </select>
      </label>
    </div>
  </section>
</template>

<script setup lang="ts">
import http from "@/http";
import { computed, onMounted, ref, watch } from "vue";
import { notify } from "@kyvg/vue3-notification";

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
const perPageOptions = [25, 50, 100];
const selected = ref<string[]>([]);
const isDeleting = ref(false);
const searchTerm = ref("");

onMounted(loadPackages);
watch(
  () => props.repositoryId,
  () => {
    packages.value = [];
    error.value = null;
    currentPage.value = 1;
    selected.value = [];
    searchTerm.value = "";
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

watch(searchTerm, () => {
  currentPage.value = 1;
});

const totalPages = computed(() => {
  if (totalPackages.value === 0) {
    return 1;
  }
  return Math.max(1, Math.ceil(totalPackages.value / perPage.value));
});

const pageLabel = computed(() => {
  if (totalPackages.value === 0) {
    return "Page 1 of 1";
  }
  const start = (currentPage.value - 1) * perPage.value + 1;
  const end = visiblePackages.value.length === 0 ? start - 1 : start + visiblePackages.value.length - 1;
  return `Page ${currentPage.value} of ${totalPages.value} · Showing ${Math.max(start, 0)}-${Math.max(end, 0)}`;
});

const normalizedSearchTerm = computed(() => searchTerm.value.trim().toLowerCase());

const visiblePackages = computed(() => {
  const term = normalizedSearchTerm.value;
  if (!term) {
    return packages.value;
  }
  return packages.value.filter((pkg) => {
    const haystack = [pkg.package, pkg.name, pkg.cachePath]
      .join(" ")
      .toLowerCase();
    return haystack.includes(term);
  });
});

const selectedCount = computed(() => selected.value.length);
const allSelected = computed(() => {
  return (
    visiblePackages.value.length > 0 &&
    visiblePackages.value.every((pkg) => selected.value.includes(pkg.cachePath))
  );
});
const isIndeterminate = computed(() => {
  const visibleSelected = visiblePackages.value.filter((pkg) =>
    selected.value.includes(pkg.cachePath),
  ).length;
  return visibleSelected > 0 && visibleSelected < visiblePackages.value.length;
});

const derivedHostedFromPackages = computed(() => {
  if (packages.value.length === 0) {
    return false;
  }
  return !packages.value.some((pkg) => pkg.cachePath.startsWith("packages/"));
});

const isHostedRepository = computed(() => {
  if (props.repositoryKind) {
    return props.repositoryKind.toLowerCase() === "hosted";
  }
  if (props.repositoryType === "python") {
    return derivedHostedFromPackages.value;
  }
  return false;
});

const headerTitle = computed(() => (isHostedRepository.value ? "Packages" : "Cached Packages"));
const pathColumnTitle = computed(() => (isHostedRepository.value ? "Path" : "Cached Path"));

const emptyRepositoryMessage = computed(() =>
  isHostedRepository.value
    ? "No packages yet. Upload a package to populate this list."
    : "No cached packages yet. Trigger a download to populate this list.",
);

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
  const count = selected.value.length;
  const confirmed = window.confirm(
    isHostedRepository.value
      ? `Delete ${count} package(s)? This removes files from the repository.`
      : `Delete ${count} cached package(s)? This removes cached files but not upstream artifacts.`,
  );
  if (!confirmed) {
    return;
  }
  isDeleting.value = true;
  try {
    await http.delete(`/api/repository/${props.repositoryId}/packages`, {
      data: { paths: selected.value },
    });
    notify({
      type: "success",
      title: "Packages deleted",
      text: isHostedRepository.value
        ? `${count} package(s) removed`
        : `${count} cached package(s) removed`,
    });
    selected.value = [];
    await loadPackages();
  } catch (err: any) {
    console.error(err);
    const message = err?.response?.data?.message ?? err?.message ?? "Failed to delete packages";
    notify({
      type: "error",
      title: "Deletion failed",
      text: message,
    });
  } finally {
    isDeleting.value = false;
  }
}

function toggleSelectAll(event: Event) {
  const target = event.target as HTMLInputElement;
  if (target.checked) {
    const visibleKeys = visiblePackages.value.map((pkg) => pkg.cachePath);
    const current = new Set(selected.value);
    visibleKeys.forEach((key) => current.add(key));
    selected.value = Array.from(current);
  } else {
    const visibleKeys = new Set(visiblePackages.value.map((pkg) => pkg.cachePath));
    selected.value = selected.value.filter((value) => !visibleKeys.has(value));
  }
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

function nextPage() {
  if (currentPage.value < totalPages.value) {
    currentPage.value += 1;
  }
}

function prevPage() {
  if (currentPage.value > 1) {
    currentPage.value -= 1;
  }
}

function updatePerPage(event: Event) {
  const target = event.target as HTMLSelectElement;
  const value = Number.parseInt(target.value, 10);
  if (!Number.isNaN(value) && perPage.value !== value) {
    perPage.value = value;
    currentPage.value = 1;
  }
}
</script>

<style scoped lang="scss">
@import "@/assets/styles/theme";

.packages {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1rem;
}

.packages__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}

.packages__title-row {
  display: flex;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}

.packages__header-meta {
  display: flex;
  align-items: center;
  gap: 1.5rem;
  flex-wrap: wrap;
}

.packages__search input {
  border: 1px solid var(--border-color, rgba(0, 0, 0, 0.15));
  border-radius: 4px;
  padding: 0.35rem 0.6rem;
  min-width: 220px;
}

.packages__counts {
  display: flex;
  gap: 0.75rem;
  color: var(--text-secondary, #6c757d);
  font-size: 0.9rem;
}

.packages__actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.packages__state {
  color: var(--text-secondary, #6c757d);
}

.packages__state--error {
  color: var(--error-color, #d9534f);
}

.packages__table {
  width: 100%;
  border-collapse: collapse;

  th,
  td {
    padding: 0.5rem;
    text-align: left;
    border-bottom: 1px solid rgba(0, 0, 0, 0.1);
  }

  code {
    font-size: 0.85rem;
  }

  .packages__checkbox {
    width: 2rem;
    text-align: center;
  }
}

.packages__pager {
  margin-top: 1rem;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  flex-wrap: wrap;
}

.packages__pager-select select {
  margin-left: 0.35rem;
}
</style>
