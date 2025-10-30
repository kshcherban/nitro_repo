<template>
  <div id="repositoryBox">
    <div id="headerBar">
      <h2>Repositories</h2>
      <input
        type="text"
        id="nameSearch"
        v-model="searchValue"
        autofocus
        placeholder="Search by Name, Storage Name" />
    </div>
      <div
        id="repositories"
        class="betterScroll">
        <section
          v-if="trimmedSearch.length >= 2"
          class="package-results">
          <header class="package-results__header">
            <h3>Package Matches</h3>
            <span v-if="!packageLoading">{{ packageResults.length }} result(s)</span>
          </header>
          <div
            v-if="packageLoading"
            class="package-results__state">
            Searching packages...
          </div>
          <div
            v-else-if="packageError"
            class="package-results__state package-results__state--error">
            {{ packageError }}
          </div>
          <div
            v-else-if="packageResults.length === 0"
            class="package-results__state">
            No packages found.
          </div>
          <table v-else class="package-results__table">
            <thead>
              <tr>
                <th>Package</th>
                <th>Repository</th>
                <th>Size</th>
                <th>Cached Path</th>
                <th>Cached At</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="pkg in packageResults"
                :key="pkg.cachePath"
                @click="openPackage(pkg)">
                <td>{{ pkg.fileName }}</td>
                <td>{{ pkg.repositoryName }} ({{ pkg.storageName }})</td>
                <td>{{ formatBytes(pkg.size) }}</td>
                <td><code>{{ pkg.cachePath }}</code></td>
                <td>{{ new Date(pkg.modified).toLocaleString() }}</td>
              </tr>
            </tbody>
          </table>
        </section>
        <div
          class="row"
          id="header">
        <div
          :class="['col', { sorted: sortBy === 'id' }]"
          @click="sortBy = 'id'"
          title="Sort by ID">
          ID #
        </div>
        <div
          :class="['col', { sorted: sortBy === 'name' }]"
          @click="sortBy = 'name'"
          title="Sort by Name">
          Name
        </div>
        <div
          :class="['col', { sorted: sortBy === 'storage-type' }]"
          @click="sortBy = 'storage-type'"
          title="Sort by Storage Type">
          Storage Name
        </div>
        <div :class="['col']">Repository Type</div>
        <div :class="['col']">Active</div>
      </div>
      <div
        class="row item"
        v-for="repository in filteredTable"
        :key="repository.id"
        @click="
          router.push({
            name: 'repository_page_by_id',
            params: { repositoryId: repository.id },
          })
        ">
        <div class="col">{{ repository.id }}</div>
        <div
          class="col"
          :title="repository.name">
          {{ repository.name }}
        </div>
        <div
          class="col"
          :title="repository.storage_name">
          {{ repository.storage_name }}
        </div>
        <div class="col">{{ repository.repository_type }}</div>
        <div class="col">{{ repository.active }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import http from "@/http";
import router from "@/router";
import type { RepositoryWithStorageName } from "@/types/repository";
import { computed, onBeforeUnmount, ref, watch, type PropType } from "vue";
const searchValue = ref<string>("");

const props = defineProps({
  repositories: Array as PropType<RepositoryWithStorageName[]>,
});
const sortBy = ref<string>("id");

interface PackageSearchResponse {
  repository_id: string;
  repository_name: string;
  storage_name: string;
  repository_type: string;
  file_name: string;
  cache_path: string;
  size: number;
  modified: string;
}

interface PackageResult {
  repositoryId: string;
  repositoryName: string;
  storageName: string;
  repositoryType: string;
  fileName: string;
  cachePath: string;
  size: number;
  modified: string;
}

const packageResults = ref<PackageResult[]>([]);
const packageLoading = ref(false);
const packageError = ref<string | null>(null);
let debounceHandle: number | undefined;

const trimmedSearch = computed(() => searchValue.value.trim());

watch(trimmedSearch, (value) => {
  packageResults.value = [];
  packageError.value = null;
  if (debounceHandle !== undefined) {
    window.clearTimeout(debounceHandle);
    debounceHandle = undefined;
  }
  if (value.length < 2) {
    packageLoading.value = false;
    return;
  }
  packageLoading.value = true;
  debounceHandle = window.setTimeout(() => {
    fetchPackages(value);
  }, 300);
});

onBeforeUnmount(() => {
  if (debounceHandle !== undefined) {
    window.clearTimeout(debounceHandle);
  }
});

async function fetchPackages(query: string) {
  try {
    const { data } = await http.get<PackageSearchResponse[]>("/api/search/packages", {
      params: { q: query, limit: 25 },
    });
    packageResults.value = data.map((item) => ({
      repositoryId: item.repository_id,
      repositoryName: item.repository_name,
      storageName: item.storage_name,
      repositoryType: item.repository_type,
      fileName: item.file_name,
      cachePath: item.cache_path,
      size: item.size,
      modified: item.modified,
    }));
  } catch (err: unknown) {
    console.error(err);
    packageError.value = "Failed to search packages";
  } finally {
    packageLoading.value = false;
  }
}

function sortList(a: RepositoryWithStorageName, b: RepositoryWithStorageName) {
  switch (sortBy.value) {
    case "id":
      return a.name.localeCompare(b.name);
    case "name":
      return a.name.localeCompare(b.name);

    default:
      return 0;
  }
}
const filteredTable = computed(() => {
  if (props.repositories == undefined) {
    return [];
  }
  const repositories = props.repositories.map((repository) => repository);
  const query = trimmedSearch.value.toLowerCase();
  const filtered = query.length
    ? repositories.filter((repository) => {
        return (
          repository.name.toLowerCase().includes(query) ||
          repository.storage_name.toLowerCase().includes(query) ||
          repository.repository_type.toLowerCase().includes(query)
        );
      })
    : repositories;
  return filtered.sort(sortList);
});

function openPackage(pkg: PackageResult) {
  const parentPath = pkg.cachePath.split("/").slice(0, -1).join("/");
  router.push({
    name: "Browse",
    params: { id: pkg.repositoryId, catchAll: parentPath },
  });
}

function formatBytes(bytes: number): string {
  if (bytes === 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  const unitIndex = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, unitIndex);
  return `${value.toFixed(unitIndex === 0 ? 0 : 2)} ${units[unitIndex]}`;
}
</script>
<style scoped lang="scss">
@import "@/assets/styles/theme";
#headerBar {
  display: flex;
  justify-content: space-between;
  padding: 1rem;
  background-color: $primary-30;
  input {
    width: 25%;
  }
}
@media screen and (max-width: 1200px) {
  #headerBar {
    input {
      width: 50%;
    }
  }
}
@media screen and (max-width: 800px) {
  #headerBar {
    display: flex;
    flex-direction: column;
    input {
      width: 100%;
    }
  }
}
#storages {
  background-color: $primary-50;
}

#repositories {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.package-results {
  padding: 1rem;
  border: 1px solid $primary-30;
  border-radius: 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.package-results__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.package-results__state {
  color: $secondary;
}

.package-results__state--error {
  color: var(--error-color, #d9534f);
}

.package-results__table {
  width: 100%;
  border-collapse: collapse;

  th,
  td {
    padding: 0.5rem;
    text-align: left;
    border-bottom: 1px solid $primary-30;
  }

  tr {
    cursor: pointer;

    &:hover {
      background-color: $primary-30;
      transition: background-color 0.2s ease;
    }
  }
}

#header {
  .col {
    font-weight: bold;
    &:hover {
      cursor: pointer;
      color: $accent;
      transition: all 0.3s ease;
    }
  }
}
.row {
  display: grid;
  grid-template-columns: 1fr 0.5fr 0.5fr 0.5fr 0.5fr;
  grid-template-rows: auto;
  .col {
    padding: 1rem;
    border-bottom: 1px solid $primary-30;
  }
}
.item {
  cursor: pointer;
  &:hover {
    background-color: $primary-30;
    transition: all 0.3s ease;
    .col {
      color: $accent;
      transition: all 0.3s ease;
    }
  }
}
</style>
