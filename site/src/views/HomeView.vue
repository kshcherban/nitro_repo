<template>
  <v-container class="pa-6">
    <!-- Hero Section -->
    <v-row class="mb-8" justify="center">
      <v-col cols="12" md="10" lg="8" class="text-center">
        <div class="d-flex flex-column align-center mb-6">
          <v-avatar
            :image="'/icon-128.png'"
            size="96"
            class="mb-4" />
          <h1 class="text-h3 font-weight-bold text-primary mb-2">Nitro Repository</h1>
          <p class="text-h6 text-medium-emphasis mb-6">
            Universal package repository manager supporting Maven, NPM, Go, Docker, Helm, Python, and PHP
          </p>
          <div class="d-flex gap-4 justify-center flex-wrap">
            <v-btn
              color="primary"
              prepend-icon="mdi-browse"
              size="large"
              variant="flat"
              :to="{ name: 'repositories' }">
              Browse Repositories
            </v-btn>
            <v-btn
              color="secondary"
              prepend-icon="mdi-magnify"
              size="large"
              variant="outlined"
              :to="{ name: 'repositories' }">
              Search Packages
            </v-btn>
          </div>
        </div>
      </v-col>
    </v-row>

    <!-- Loading State -->
    <v-card v-if="loading && !error" class="text-center py-8">
      <v-progress-circular indeterminate color="primary" size="48" />
      <div class="mt-4 text-medium-emphasis">Loading repositories…</div>
    </v-card>

    <!-- Error State -->
    <v-alert
      v-else-if="error"
      type="error"
      variant="tonal"
      prominent>
      Failed to load repositories: {{ error }}
    </v-alert>

    <!-- Repository Grid -->
    <div v-else-if="repositories.length >= 1">
      <v-row class="align-center mb-6">
        <v-col>
          <h2 class="text-h4 font-weight-medium">Repositories</h2>
          <p class="text-body-1 text-medium-emphasis">
            Browse {{ repositories.length }} available repositories
          </p>
        </v-col>
        <v-col cols="auto">
          <v-text-field
            v-model="searchTerm"
            label="Search repositories..."
            prepend-inner-icon="mdi-magnify"
            variant="outlined"
            clearable
            @click:clear="clearSearchTerm"
            hide-details
            style="min-width: 300px;" />
        </v-col>
      </v-row>

      <v-row>
        <v-col
          v-for="repo in filteredRepositories"
          :key="repo.id"
          cols="12"
          sm="6"
          md="4"
          lg="3">
          <v-card
            :ripple="false"
            class="repository-card h-100"
            @click="navigateToRepository(repo)">
            <v-card-title class="d-flex align-center pa-4">
              <span class="repository-card__icon mr-3">
                <component
                  v-if="hasComponentIcon(repo.repository_type || '')"
                  :is="getComponentIcon(repo.repository_type || '').component"
                  v-bind="getComponentIcon(repo.repository_type || '').props"
                  class="repository-card__brand-icon" />
                <v-icon
                  v-else
                  :icon="getFallbackIcon(repo.repository_type || '')"
                  color="primary" />
              </span>
              <div>
                <div class="text-h6">{{ repo.name || 'Unknown' }}</div>
                <div class="text-caption text-medium-emphasis">
                  {{ (repo.repository_type || '').toUpperCase() }}
                </div>
              </div>
            </v-card-title>

            <v-card-text class="pa-4 pt-0">
              <div class="d-flex align-center gap-4 text-caption">
                <div class="d-flex align-center gap-1">
                  <v-icon size="small">mdi-database</v-icon>
                  {{ repo.storage_name || 'Unknown' }}
                </div>
                <div class="d-flex align-center gap-1">
                  <v-icon size="small">mdi-shield-check</v-icon>
                  <span :class="repo.auth_enabled ? 'text-success' : 'text-medium-emphasis'">
                    {{ repo.auth_enabled ? 'Secured' : 'Public' }}
                  </span>
                </div>
              </div>

              <div v-if="repo.storage_usage_bytes !== undefined" class="mt-2">
                <div class="d-flex align-center gap-1 text-caption">
                  <v-icon size="small">mdi-hard-disk</v-icon>
                  {{ formatBytes(repo.storage_usage_bytes) }}
                </div>
              </div>
            </v-card-text>

            <v-card-actions class="pa-4 pt-0">
              <v-btn
                color="primary"
                variant="text"
                prepend-icon="mdi-browse"
                class="text-none">
                Browse
              </v-btn>
              <v-spacer />
              <v-chip
                :color="repo.active ? 'success' : 'default'"
                :variant="repo.active ? 'flat' : 'outlined'"
                size="small">
                {{ repo.active ? 'Active' : 'Inactive' }}
              </v-chip>
            </v-card-actions>
          </v-card>
        </v-col>
      </v-row>
    </div>

    <!-- Empty State -->
    <v-card
      v-else
      class="text-center py-12"
      variant="outlined">
      <v-icon color="medium-emphasis" size="64" class="mb-4">mdi-package-variant</v-icon>
      <h2 class="text-h4 text-medium-emphasis mb-2">No repositories available</h2>
      <p class="text-body-1 text-medium-emphasis mb-6">
        Contact your administrator to create repositories.
      </p>
      <v-btn
        v-if="user?.admin"
        color="primary"
        prepend-icon="mdi-plus"
        :to="{ name: 'AdminCreateRepository' }"
        variant="flat">
        Create Repository
      </v-btn>
    </v-card>
  </v-container>
</template>

<script setup lang="ts">
import { useRouter } from "vue-router";
import { computed, onMounted, ref } from "vue";
import type { Component } from "vue";
import { useRepositoryStore } from "@/stores/repositories";
import { sessionStore } from "@/stores/session";
import type { RepositoryWithStorageName } from "@/types/repository";
import { HelmIcon, DockerIcon } from "vue3-simple-icons";

const router = useRouter();
const repositories = ref<RepositoryWithStorageName[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);
const searchTerm = ref("");
const repoStore = useRepositoryStore();
const session = sessionStore();
const user = computed(() => session.user);

type ComponentIcon = {
  component: Component;
  props?: Record<string, unknown>;
};

const componentIconMap: Record<string, ComponentIcon> = {
  helm: {
    component: HelmIcon,
    props: {
      size: "32",
      color: "#0F1689",
    },
  },
  docker: {
    component: DockerIcon,
    props: {
      size: "32",
      color: "#2496ED",
    },
  },
};

const fallbackIconMap: Record<string, string> = {
  maven: "mdi-language-java",
  npm: "mdi-nodejs",
  go: "mdi-language-go",
  python: "mdi-language-python",
  php: "mdi-language-php",
};

function normalizeType(type: string): string {
  return type?.toLowerCase?.() ?? "";
}

function hasComponentIcon(type: string): boolean {
  return Boolean(componentIconMap[normalizeType(type)]);
}

function getComponentIcon(type: string): ComponentIcon {
  return componentIconMap[normalizeType(type)]!;
}

function getFallbackIcon(type: string): string {
  const normalized = normalizeType(type);
  return fallbackIconMap[normalized] ?? "mdi-package-variant";
}

// Filter repositories based on search term
const filteredRepositories = computed(() => {
  if (!searchTerm.value?.trim()) {
    return repositories.value || [];
  }
  const term = searchTerm.value.toLowerCase();
  return (repositories.value || []).filter(repo =>
    repo?.name?.toLowerCase().includes(term) ||
    repo?.repository_type?.toLowerCase().includes(term) ||
    repo?.storage_name?.toLowerCase().includes(term)
  );
});

// Format bytes for display
function formatBytes(bytes?: number | null): string {
  if (bytes === null || bytes === undefined) {
    return "—";
  }
  if (bytes === 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, exponent);
  return `${value.toFixed(exponent === 0 ? 0 : 2)} ${units[exponent]}`;
}

// Navigate to repository
function navigateToRepository(repo: RepositoryWithStorageName) {
  router.push({
    name: 'Browse',
    params: { id: repo.id, catchAll: '' }
  });
}

async function getRepositories() {
  loading.value = true;
  error.value = null;
  try {
    const response = await repoStore.getRepositories();
    repositories.value = Array.isArray(response) ? response : [];
  } catch (err) {
    console.error(err);
    error.value = "Failed to load repositories";
    repositories.value = [];
  } finally {
    loading.value = false;
  }
}

onMounted(getRepositories);

function clearSearchTerm() {
  searchTerm.value = "";
}
</script>

<style scoped lang="scss">
.repository-card {
  cursor: pointer;
  transition: transform 0.2s ease-in-out;

  &:hover {
    transform: translateY(-4px);
  }
}

.repository-card__brand-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
}

.repository-card__brand-icon :deep(svg) {
  width: 32px;
  height: 32px;
}

:deep(.v-card--hover) {
  cursor: pointer;
}
</style>
