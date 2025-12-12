<template>
  <div id="repositoryBox" class="public-repository-list">
    <RepositorySearchHeader
      v-model="searchValue"
      autofocus />
    <PackageSearchResults
      v-if="showPackageResults"
      :results="packageResults"
      :loading="packageLoading"
      :error="packageError"
      @open="openPackage" />
    <div
      id="repositories"
      class="betterScroll">
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
        <div :class="['col']">Kind</div>
        <div :class="['col']">Active</div>
      </div>
      <div
        class="row item"
        v-for="repository in filteredTable"
        :key="repository.id"
        @click="
          router.push({
            name: 'Browse',
            params: { id: repository.id },
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
        <div class="col">{{ repositoryTypeLabel(repository) }}</div>
        <div class="col">
          <v-chip
            size="small"
            :color="kindColor(repository)"
            variant="tonal">
            {{ repositoryKindLabel(repository) }}
          </v-chip>
        </div>
        <div class="col">{{ repository.active }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import PackageSearchResults, { type PackageResult } from "@/components/nr/repository/PackageSearchResults.vue";
import RepositorySearchHeader from "@/components/nr/repository/RepositorySearchHeader.vue";
import http from "@/http";
import router from "@/router";
import type { RepositoryWithStorageName } from "@/types/repository";
import { isAdvancedQuery, shouldFetchPackages } from "@/utils/repositorySearch";
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

const packageResults = ref<PackageResult[]>([]);
const packageLoading = ref(false);
const packageError = ref<string | null>(null);
let debounceHandle: number | undefined;

const trimmedSearch = computed(() => searchValue.value.trim());
const showPackageResults = computed(() => shouldFetchPackages(trimmedSearch.value));

watch(trimmedSearch, (value) => {
  packageResults.value = [];
  packageError.value = null;
  if (debounceHandle !== undefined) {
    window.clearTimeout(debounceHandle);
    debounceHandle = undefined;
  }
  if (!shouldFetchPackages(value)) {
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
    const response = await http.get<PackageSearchResponse[]>("/api/search/packages", {
      params: { q: query, limit: 25 },
    });
    packageResults.value = response.data.map((item) => ({
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

function repositoryKindLabel(repo: RepositoryWithStorageName) {
  const kind = (repo.repository_kind ?? "hosted").toString().toLowerCase();
  if (kind === "proxy") return "Proxy";
  if (kind === "virtual") return "Virtual";
  return "Hosted";
}

function repositoryTypeLabel(repo: RepositoryWithStorageName) {
  const kind = repositoryKindLabel(repo).toLowerCase();
  return repo.repository_type.toLowerCase() === "docker"
    ? `docker (${kind})`
    : repo.repository_type;
}

function kindColor(repo: RepositoryWithStorageName) {
  return repositoryKindLabel(repo) === "Proxy" ? "primary" : "default";
}

const filteredTable = computed(() => {
  if (!props.repositories) {
    return [];
  }
  const repositories = props.repositories.map((repository) => repository);
  const rawQuery = trimmedSearch.value;
  const loweredQuery = rawQuery.toLowerCase();
  const advanced = isAdvancedQuery(rawQuery);
  const filtered = loweredQuery.length && !advanced
    ? repositories.filter((repository) => {
        return (
          repository.name.toLowerCase().includes(loweredQuery) ||
          repository.storage_name.toLowerCase().includes(loweredQuery) ||
          repository.repository_type.toLowerCase().includes(loweredQuery)
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
</script>

<style scoped lang="scss">
@use "@/assets/styles/theme" as *;

.public-repository-list {
  padding: 0 1.5rem;
}

#repositoryBox {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

#repositories {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

#header .col {
  font-weight: bold;
  cursor: pointer;
  transition: color 0.2s ease;

  &:hover {
    color: $accent;
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
  transition: background-color 0.2s ease;

  &:hover {
    background-color: $primary-30;

    .col {
      color: $accent;
      transition: color 0.2s ease;
    }
  }
}

@media (max-width: 900px) {
  .row {
    grid-template-columns: 1fr;
  }
}
</style>
