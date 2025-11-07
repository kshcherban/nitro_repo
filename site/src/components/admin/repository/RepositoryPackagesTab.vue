<template>
  <section class="packages">
    <header class="packages__header">
      <div class="packages__title-row">
        <h2>{{ headerTitle }}</h2>
        <div
          class="packages__search"
          v-if="!isLoading && totalPackages > 0">
          <input
            type="search"
            :placeholder="`Search ${headerTitle.toLowerCase()}…`"
            v-model="searchTerm"
            aria-label="Search packages" />
        </div>
      </div>
      <div
        v-if="!isLoading"
        class="packages__header-meta">
        <div class="packages__counts">
          <span>{{ totalPackages }} package(s)</span>
          <span v-if="visiblePackages.length"> Showing {{ visiblePackages.length }} file(s) </span>
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

    <div
      v-if="isLoading"
      class="packages__state">
      Loading packages...
    </div>
    <div
      v-else-if="error"
      class="packages__state packages__state--error">
      Failed to load packages: {{ error }}
    </div>
    <div
      v-else-if="totalPackages === 0"
      class="packages__state">
      {{ emptyRepositoryMessage }}
    </div>
    <div
      v-else-if="visiblePackages.length === 0"
      class="packages__state">
      No packages match your search on this page. Try a different page or clear the filters.
    </div>
    <div
      v-else
      class="packages__table-container">
      <table class="packages__table">
        <thead>
          <tr>
            <th
              class="packages__checkbox"
              data-column="checkbox">
              <input
                type="checkbox"
                :checked="allSelected"
                :indeterminate.prop="isIndeterminate"
                @change="toggleSelectAll"
                :disabled="isDeleting" />
            </th>
            <th
              :style="{ width: columnWidths.package + 'px' }"
              class="resizable"
              data-column="package"
              @mousedown="startResize($event, 'package')">
              <div class="column-header">
                {{ packageColumnTitle }}
                <div class="resize-handle"></div>
              </div>
            </th>
            <th
              :style="{ width: columnWidths.name + 'px' }"
              class="resizable"
              data-column="name"
              @mousedown="startResize($event, 'name')">
              <div class="column-header">
                {{ nameColumnTitle }}
                <div class="resize-handle"></div>
              </div>
            </th>
            <th
              :style="{ width: columnWidths.size + 'px' }"
              class="resizable"
              data-column="size"
              @mousedown="startResize($event, 'size')">
              <div class="column-header">
                Size
                <div class="resize-handle"></div>
              </div>
            </th>
            <th
              :style="{ width: columnWidths.path + 'px' }"
              class="resizable"
              data-column="path"
              @mousedown="startResize($event, 'path')">
              <div class="column-header">
                {{ pathColumnTitle }}
                <div class="resize-handle"></div>
              </div>
            </th>
            <th
              :style="{ width: columnWidths.timestamp + 'px' }"
              class="resizable"
              data-column="timestamp"
              @mousedown="startResize($event, 'timestamp')">
              <div class="column-header">
                {{ timestampColumnTitle }}
                <div class="resize-handle"></div>
              </div>
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="pkg in visiblePackages"
            :key="pkg.cachePath">
            <td class="packages__checkbox">
              <input
                type="checkbox"
                :value="pkg.cachePath"
                v-model="selected"
                :disabled="isDeleting" />
            </td>
            <td class="package-cell">
              <div
                class="cell-content"
                :title="pkg.package">
                {{ pkg.package }}
              </div>
            </td>
            <td class="name-cell">
              <div
                class="cell-content"
                :title="pkg.name">
                {{ pkg.name }}
              </div>
            </td>
            <td class="size-cell">
              <div class="cell-content">{{ formatBytes(pkg.size) }}</div>
            </td>
            <td class="path-cell">
              <div class="cell-content">
                <code :title="pkg.cachePath">{{ pkg.cachePath }}</code>
              </div>
            </td>
            <td class="timestamp-cell">
              <div
                class="cell-content"
                :title="new Date(pkg.modified).toLocaleString()">
                {{ new Date(pkg.modified).toLocaleString() }}
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    <div
      v-if="totalPackages > 0"
      class="packages__pager">
      <button
        class="nr-button"
        type="button"
        @click="prevPage"
        :disabled="currentPage === 1">
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
        <select
          :value="perPage"
          @change="updatePerPage">
          <option
            v-for="option in perPageOptions"
            :key="option"
            :value="option">
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

// Column resizing state
const columnWidths = ref({
  package: 150,
  name: 200,
  size: 80,
  path: 300,
  timestamp: 180,
});
const isResizing = ref(false);
const resizingColumn = ref<string | null>(null);
const startX = ref(0);
const startWidth = ref(0);

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
  const end =
    visiblePackages.value.length === 0 ? start - 1 : start + visiblePackages.value.length - 1;
  return `Page ${currentPage.value} of ${totalPages.value} · Showing ${Math.max(start, 0)}-${Math.max(end, 0)}`;
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

const isDockerRepository = computed(() => {
  const type = props.repositoryType?.toLowerCase();
  return type === "docker";
});

const isHostedRepository = computed(() => {
  if (props.repositoryKind) {
    return props.repositoryKind.toLowerCase() === "hosted";
  }
  if (isDockerRepository.value) {
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
  return isHostedRepository.value ? "Packages" : "Cached Packages";
});
const packageColumnTitle = computed(() => (isDockerRepository.value ? "Repository" : "Package"));
const nameColumnTitle = computed(() => (isDockerRepository.value ? "Tag" : "Name"));
const pathColumnTitle = computed(() => {
  if (isDockerRepository.value) {
    return "Manifest Path";
  }
  return isHostedRepository.value ? "Path" : "Cached Path";
});
const timestampColumnTitle = computed(() =>
  isDockerRepository.value ? "Uploaded At" : "Cached At",
);

const emptyRepositoryMessage = computed(() => {
  if (isDockerRepository.value) {
    return "No images yet. Push an image to populate this list.";
  }
  return isHostedRepository.value
    ? "No packages yet. Upload a package to populate this list."
    : "No cached packages yet. Trigger a download to populate this list.";
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
    notify({
      type: "success",
      title: successTitle,
      text: successText,
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

function startResize(event: MouseEvent, column: string) {
  event.preventDefault();
  isResizing.value = true;
  resizingColumn.value = column;
  startX.value = event.clientX;
  startWidth.value = columnWidths.value[column as keyof typeof columnWidths.value];

  // Add global mouse event listeners
  document.addEventListener("mousemove", handleResize);
  document.addEventListener("mouseup", stopResize);
}

function handleResize(event: MouseEvent) {
  if (!isResizing.value || !resizingColumn.value) return;

  const diff = event.clientX - startX.value;
  const newWidth = Math.max(50, startWidth.value + diff); // Minimum width of 50px
  columnWidths.value[resizingColumn.value as keyof typeof columnWidths.value] = newWidth;
}

function stopResize() {
  isResizing.value = false;
  resizingColumn.value = null;

  // Remove global mouse event listeners
  document.removeEventListener("mousemove", handleResize);
  document.removeEventListener("mouseup", stopResize);
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
  border: 1px solid var(--nr-input-border, rgba(0, 0, 0, 0.25));
  border-radius: 6px;
  padding: 0.35rem 0.6rem;
  min-width: 220px;
  background: var(--nr-input-background, rgba(26, 33, 58, 0.92));
  color: var(--nr-text-color, #f8f9fa);
  transition:
    border-color 0.2s ease,
    box-shadow 0.2s ease;
  &::placeholder {
    color: var(--nr-input-placeholder, rgba(226, 230, 246, 0.55));
  }
  &:focus {
    outline: none;
    border-color: var(--nr-primary-color, #8aa3db);
    box-shadow: 0 0 0 2px var(--nr-focus-ring, rgba(138, 163, 219, 0.35));
  }
}

.packages__counts {
  display: flex;
  gap: 0.75rem;
  color: var(--nr-text-secondary, #6c757d);
  font-size: 0.9rem;
}

.packages__actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.packages__state {
  color: var(--nr-text-secondary, #6c757d);
}

.packages__state--error {
  color: var(--error-color, #d9534f);
}

.packages__table-container {
  width: 100%;
  overflow-x: auto;
  border: 1px solid var(--nr-border-color, rgba(0, 0, 0, 0.15));
  border-radius: 6px;
  background: var(--nr-background-secondary, #f8f9fa);
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.25);
}

.packages__table {
  width: 100%;
  min-width: 800px; /* Minimum width before horizontal scroll */
  border-collapse: collapse;
  table-layout: fixed;
  color: var(--nr-text-color, inherit);

  th,
  td {
    padding: 0.5rem;
    text-align: left;
    border-bottom: 1px solid var(--nr-border-color, rgba(0, 0, 0, 0.1));
    overflow: hidden;
  }

  th {
    background: var(--nr-background-tertiary, #f8f9fa);
    font-weight: 600;
    position: relative;
    user-select: none;
    color: var(--nr-text-color, inherit);
    border-bottom: 2px solid var(--nr-border-color, rgba(0, 0, 0, 0.15));
  }
  tbody tr {
    background: var(--nr-background-primary, transparent);
    transition: background-color 0.2s ease;
  }
  tbody tr:hover {
    background: var(--nr-table-row-hover, rgba(138, 163, 219, 0.08));
  }

  .packages__checkbox {
    width: 40px;
    text-align: center;
    min-width: 40px;
    max-width: 40px;
  }

  .resizable {
    cursor: col-resize;
  }

  .column-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 100%;
    padding-right: 8px;
  }

  .resize-handle {
    width: 4px;
    height: 100%;
    background: transparent;
    cursor: col-resize;
    position: absolute;
    right: 0;
    top: 0;
    bottom: 0;
    transition: background-color 0.2s;
  }

  .resize-handle:hover {
    background: var(--nr-primary-color, #007bff);
  }

  .resizable:hover .resize-handle {
    background: var(--nr-primary-color, #007bff);
  }

  code {
    font-size: 0.85rem;
    background: var(--nr-background-tertiary, #f8f9fa);
    color: var(--nr-text-color, inherit);
    padding: 2px 4px;
    border-radius: 3px;
    display: inline-block;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border: 1px solid var(--nr-border-color, rgba(0, 0, 0, 0.1));
  }

  /* Cell styling for better text handling */
  .cell-content {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.4;
    color: var(--nr-text-color, inherit);
  }

  .package-cell .cell-content {
    font-weight: 500;
  }

  .size-cell .cell-content {
    text-align: right;
  }

  .timestamp-cell .cell-content {
    font-size: 0.9rem;
  }

  // Ensure table cells inherit proper colors
  td {
    color: var(--nr-text-color, inherit);
    background: transparent;
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
