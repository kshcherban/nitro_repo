<template>
  <div id="repositoryBox">
    <div id="headerBar">
      <h2>Repositories</h2>
      <div class="search-container">
        <div class="search-container__input">
          <v-text-field
            v-model="searchValue"
            data-testid="repository-search-input"
            class="repository-search"
            placeholder="Search packages or repositories"
            aria-label="Search repositories or packages"
            variant="outlined"
            density="comfortable"
            clearable
            hide-details
            autofocus
            prepend-inner-icon="mdi-magnify"
            @click:clear="clearTopLevelSearch" />
        </div>
        <button
          type="button"
          class="search-help-button"
          data-testid="search-help-button"
          @click="toggleSearchHelp"
          :aria-expanded="showSearchHelp"
          aria-controls="search-help-modal"
          title="Search syntax help">
          <span class="sr-only">Search syntax help</span>
          ?
        </button>
      </div>
    </div>
    <div
      v-if="showSearchHelp"
      class="search-help-overlay"
      data-testid="search-help-modal"
      role="dialog"
      aria-modal="true"
      aria-labelledby="searchHelpTitle"
      @click.self="closeSearchHelp">
      <div class="search-help-modal">
        <header class="search-help-modal__header">
          <h3 id="searchHelpTitle">Search Syntax Guide</h3>
          <button
            type="button"
            class="modal-close"
            @click="closeSearchHelp"
            aria-label="Close search help">
            ×
          </button>
        </header>
        <div class="search-help-modal__content">
          <section>
            <h4>Basic Search</h4>
            <p>Enter any text to match package names, versions, repository names, or storage names.</p>
            <button
              type="button"
              class="example-chip"
              data-testid="search-example-basic"
              @click="applyExample('gin')">
              <code>gin</code>
              <span>Simple text search</span>
            </button>
          </section>
          <section>
            <h4>Field Filters</h4>
            <table class="syntax-table">
              <thead>
                <tr>
                  <th>Field</th>
                  <th>Description</th>
                  <th>Example</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td><code>package:</code>, <code>pkg:</code></td>
                  <td>Filter by package name</td>
                  <td><code>package:express</code></td>
                </tr>
                <tr>
                  <td><code>version:</code>, <code>v:</code></td>
                  <td>Filter by version</td>
                  <td><code>version:&gt;=1.0.0</code></td>
                </tr>
                <tr>
                  <td><code>repository:</code>, <code>repo:</code></td>
                  <td>Filter by repository name</td>
                  <td><code>repo:npm-hosted</code></td>
                </tr>
                <tr>
                  <td><code>type:</code></td>
                  <td>Filter by repository type</td>
                  <td><code>type:helm</code></td>
                </tr>
                <tr>
                  <td><code>storage:</code></td>
                  <td>Filter by storage name</td>
                  <td><code>storage:primary</code></td>
                </tr>
              </tbody>
            </table>
          </section>
          <section>
            <h4>Version Operators</h4>
            <ul class="operator-list">
              <li><code>&gt;</code>, <code>&gt;=</code>, <code>&lt;</code>, <code>&lt;=</code> — numeric or semantic comparisons</li>
              <li><code>=</code> — exact match (default)</li>
              <li><code>~</code> — contains search (e.g., <code>version:~beta</code>)</li>
              <li><code>^</code>, <code>~</code> prefix — semantic ranges (e.g., <code>version:^1.5</code>)</li>
            </ul>
          </section>
          <section>
            <h4>Examples</h4>
            <div class="examples-grid">
              <button
                type="button"
                class="example-card"
                data-testid="search-example-version"
                @click="applyExample('package:gin version:>1.0')">
                <code>package:gin version:&gt;1.0</code>
                <span>Gin packages newer than 1.0</span>
              </button>
              <button
                type="button"
                class="example-card"
                @click="applyExample('package:spring type:maven')">
                <code>package:spring type:maven</code>
                <span>Spring artifacts in Maven repositories</span>
              </button>
              <button
                type="button"
                class="example-card"
                @click="applyExample('nginx repo:docker-prod')">
                <code>nginx repo:docker-prod</code>
                <span>Nginx images in docker-prod</span>
              </button>
              <button
                type="button"
                class="example-card"
                @click="applyExample('package:~express version:>=4.0.0 type:npm')">
                <code>package:~express version:&gt;=4.0.0 type:npm</code>
                <span>Express packages ≥ 4.0.0 in npm repos</span>
              </button>
            </div>
          </section>
          <section>
            <h4>Tips</h4>
            <ul class="tips-list">
              <li>Combine multiple filters with spaces, e.g., <code>repo:npm-prod version:&gt;=2.0</code>.</li>
              <li>Use quotes for values containing spaces: <code>package:"@scope/pkg"</code>.</li>
              <li>Filters are case-insensitive.</li>
            </ul>
          </section>
        </div>
      </div>
    </div>
    <div
      id="repositories"
      class="betterScroll">
      <section
          v-if="showPackageResults"
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
import { computed, onBeforeUnmount, onMounted, ref, watch, type PropType } from "vue";
const searchValue = ref<string>("");
const showSearchHelp = ref(false);

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
let keydownHandler: ((event: KeyboardEvent) => void) | null = null;

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
  if (keydownHandler) {
    window.removeEventListener("keydown", keydownHandler);
  }
});

onMounted(() => {
  keydownHandler = (event: KeyboardEvent) => {
    if (event.key === "Escape" && showSearchHelp.value) {
      showSearchHelp.value = false;
    }
  };
  window.addEventListener("keydown", keydownHandler);
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

function formatBytes(bytes: number): string {
  if (bytes === 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  const unitIndex = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, unitIndex);
  return `${value.toFixed(unitIndex === 0 ? 0 : 2)} ${units[unitIndex]}`;
}

function toggleSearchHelp() {
  showSearchHelp.value = !showSearchHelp.value;
}

function closeSearchHelp() {
  showSearchHelp.value = false;
}

function applyExample(query: string) {
  searchValue.value = query;
  showSearchHelp.value = false;
}

function clearTopLevelSearch() {
  searchValue.value = "";
}

function shouldFetchPackages(value: string): boolean {
  const trimmed = value.trim();
  if (trimmed.length >= 2) {
    return true;
  }
  return isAdvancedQuery(trimmed);
}

function isAdvancedQuery(value: string): boolean {
  return /[a-zA-Z]+:/.test(value);
}
</script>
<style scoped lang="scss">
@use "@/assets/styles/theme" as *;
#headerBar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
  padding: 1rem;
  background-color: $primary-30;
}

.search-container {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}

.search-container__input {
  position: relative;
  flex: 1 1 40rem;
  max-width: 48rem;
  min-width: 24rem;
}

.repository-search {
  width: 100%;
  max-width: none;
}

.search-help-button {
  width: 2.5rem;
  height: 2.5rem;
  border-radius: 50%;
  border: 1px solid $primary-50;
  background: var(--nr-background-primary, #fff);
  color: $accent;
  font-weight: 600;
  font-size: 1.2rem;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background-color 0.2s ease, color 0.2s ease, transform 0.2s ease;
}

.search-help-button:hover,
.search-help-button:focus-visible {
  background: $accent;
  color: var(--nr-background-primary, #fff);
  transform: translateY(-1px);
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

.search-help-overlay {
  position: fixed;
  inset: 0;
  background: rgba(17, 24, 39, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1.25rem;
  z-index: 30;
}

.search-help-modal {
  background: var(--nr-background-primary, #fff);
  color: var(--nr-text-color, #1f2937);
  border-radius: 0.75rem;
  box-shadow: 0 20px 45px rgba(15, 23, 42, 0.25);
  width: min(48rem, 100%);
  max-height: 85vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
  padding: 1.5rem;
}

.search-help-modal__header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
}

.search-help-modal__content {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.modal-close {
  border: none;
  background: transparent;
  font-size: 1.5rem;
  cursor: pointer;
  color: inherit;
  transition: transform 0.2s ease, color 0.2s ease;
}

.modal-close:hover,
.modal-close:focus-visible {
  transform: scale(1.1);
  color: $accent;
}

.syntax-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.95rem;
}

.syntax-table th,
.syntax-table td {
  padding: 0.5rem;
  border-bottom: 1px solid rgba(15, 23, 42, 0.12);
  text-align: left;
}

.syntax-table code {
  background: rgba(15, 23, 42, 0.08);
  padding: 0.2rem 0.35rem;
  border-radius: 0.25rem;
}

.examples-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 0.75rem;
}

.example-card,
.example-chip {
  border: 1px solid rgba(15, 23, 42, 0.12);
  background: var(--nr-background-secondary, #f8fafc);
  border-radius: 0.5rem;
  padding: 0.75rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  text-align: left;
  cursor: pointer;
  transition: border-color 0.2s ease, box-shadow 0.2s ease, transform 0.2s ease;
  color: inherit;
}

.example-chip {
  display: inline-flex;
  flex-direction: row;
  align-items: center;
  gap: 0.5rem;
  width: fit-content;
}

.example-card:hover,
.example-card:focus-visible,
.example-chip:hover,
.example-chip:focus-visible {
  border-color: $accent;
  box-shadow: 0 0 0 3px rgba(100, 116, 139, 0.15);
  transform: translateY(-1px);
}

.example-card code,
.example-chip code {
  background: rgba(15, 23, 42, 0.08);
  padding: 0.25rem 0.4rem;
  border-radius: 0.25rem;
  font-size: 0.85rem;
}

.operator-list,
.tips-list {
  margin: 0;
  padding-left: 1.25rem;
  display: flex;
  flex-direction: column;
  gap: 0.35rem;
  font-size: 0.95rem;
}

@media screen and (max-width: 1200px) {
  .search-container__input {
    max-width: none;
    min-width: 0;
  }
}

@media screen and (max-width: 900px) {
  #headerBar {
    flex-direction: column;
    align-items: flex-start;
  }
  .search-container {
    width: 100%;
  }
  .search-container__input {
    flex: 1 1 100%;
    width: 100%;
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
