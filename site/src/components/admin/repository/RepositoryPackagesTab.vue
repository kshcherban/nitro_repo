<template>
  <section class="packages">
    <header class="packages__header">
      <h2>Cached Packages</h2>
      <span v-if="!isLoading">{{ packages.length }} item(s)</span>
    </header>

    <div v-if="isLoading" class="packages__state">Loading packages...</div>
    <div v-else-if="error" class="packages__state packages__state--error">
      Failed to load packages: {{ error }}
    </div>
    <div v-else-if="packages.length === 0" class="packages__state">
      No cached packages yet. Trigger a download to populate this list.
    </div>
    <table v-else class="packages__table">
      <thead>
        <tr>
          <th>Name</th>
          <th>Size</th>
          <th>Cached Path</th>
          <th>Cached At</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="pkg in packages" :key="pkg.cachePath">
          <td>{{ pkg.name }}</td>
          <td>{{ formatBytes(pkg.size) }}</td>
          <td><code>{{ pkg.cachePath }}</code></td>
          <td>{{ new Date(pkg.modified).toLocaleString() }}</td>
        </tr>
      </tbody>
    </table>
  </section>
</template>

<script setup lang="ts">
import http from "@/http";
import { ref, watch, onMounted } from "vue";
import type { RawBrowseFile } from "@/types/browse";
import { isAxiosError } from "axios";

interface PackageEntry {
  name: string;
  size: number;
  cachePath: string;
  modified: string;
}

const props = defineProps<{ repositoryId: string }>();

const packages = ref<PackageEntry[]>([]);
const isLoading = ref(false);
const error = ref<string | null>(null);

onMounted(loadPackages);
watch(
  () => props.repositoryId,
  () => {
    packages.value = [];
    error.value = null;
    loadPackages();
  },
);

async function loadPackages() {
  if (!props.repositoryId) {
    return;
  }
  isLoading.value = true;
  error.value = null;
  try {
    const collected: PackageEntry[] = [];
    await traverse("packages", collected);
    packages.value = collected.sort((a, b) => a.name.localeCompare(b.name));
  } catch (err) {
    console.error(err);
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    isLoading.value = false;
  }
}

async function traverse(path: string, collected: PackageEntry[]) {
  let response;
  try {
    response = await http.get(`/api/repository/browse/${props.repositoryId}/${path}`, {
      params: { check_for_project: false },
    });
  } catch (err) {
    if (isAxiosError(err) && err.response?.status === 404) {
      return;
    }
    throw err;
  }
  const files: RawBrowseFile[] = response.data.files ?? [];
  for (const entry of files) {
    if (entry.type === "Directory") {
      await traverse(joinPath(path, entry.value.name), collected);
    } else {
      const name = entry.value.name;
      if (shouldIgnore(name)) {
        continue;
      }
      collected.push({
        name,
        size: entry.value.file_size,
        cachePath: joinPath(path, name),
        modified: entry.value.modified,
      });
    }
  }
}

function joinPath(base: string, segment: string): string {
  if (!base) {
    return segment;
  }
  return `${base.replace(/\/+$/g, "")}/${segment}`;
}

function shouldIgnore(name: string): boolean {
  return name.startsWith(".") || name.endsWith(".nr-meta");
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
</style>
