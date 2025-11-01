<template>
  <section class="packages">
    <header class="packages__header">
      <h2>Cached Packages</h2>
      <div v-if="!isLoading" class="packages__counts">
        <span>{{ totalPackages }} package(s)</span>
        <span v-if="packages.length">Showing {{ packages.length }} file(s)</span>
      </div>
    </header>

    <div v-if="isLoading" class="packages__state">Loading packages…</div>
    <div v-else-if="error" class="packages__state packages__state--error">
      Failed to load packages: {{ error }}
    </div>
    <div v-else-if="totalPackages === 0" class="packages__state">
      No cached packages yet. Trigger a download to populate this list.
    </div>
    <div v-else-if="packages.length === 0" class="packages__state">
      No packages on this page. Try a different page.
    </div>
    <table v-else class="packages__table">
      <thead>
        <tr>
          <th>Package</th>
          <th>Name</th>
          <th>Size</th>
          <th>Cached Path</th>
          <th>Cached At</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="pkg in packages" :key="pkg.cachePath">
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

interface PackageEntry {
  name: string;
  size: number;
  cachePath: string;
  modified: string;
  package: string;
}

const props = defineProps<{ repositoryId: string }>();

const packages = ref<PackageEntry[]>([]);
const isLoading = ref(false);
const error = ref<string | null>(null);
const currentPage = ref(1);
const perPage = ref(25);
const totalPackages = ref(0);
const perPageOptions = [25, 50, 100];

onMounted(loadPackages);

watch(
  () => props.repositoryId,
  () => {
    packages.value = [];
    error.value = null;
    currentPage.value = 1;
    loadPackages();
  },
);

watch([currentPage, perPage], () => {
  if (!props.repositoryId) {
    return;
  }
  loadPackages();
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
  const end = packages.value.length === 0 ? start - 1 : start + packages.value.length - 1;
  return `Page ${currentPage.value} of ${totalPages.value} · Showing ${Math.max(start, 0)}-${Math.max(end, 0)}`;
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
  } catch (err) {
    console.error(err);
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    isLoading.value = false;
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
  padding: 1rem 0;
}

.packages__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}

.packages__counts {
  display: flex;
  gap: 0.75rem;
  color: var(--text-secondary, #6c757d);
  font-size: 0.9rem;
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
