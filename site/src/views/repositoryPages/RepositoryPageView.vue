<template>
  <v-container v-if="repository" class="repository-page" fluid>
    <v-card variant="flat" class="repository-page__header">
      <v-card-text class="repository-page__header-content">
        <div class="repository-page__title">
          <h1 class="text-h5 text-md-h4 font-weight-semibold mb-1">
            {{ repository.storage_name }}/{{ repository.name }}
          </h1>
          <div class="repository-page__meta">
            <CopyURL :code="url" />
            <div
              v-if="repositoryType"
              class="repository-page__icons">
              <RepositoryIcon
                v-for="icon in repositoryType.icons"
                :key="icon.name"
                :name="repositoryType.name"
                :icon="icon" />
            </div>
          </div>
        </div>
        <v-btn
          color="primary"
          variant="tonal"
          class="text-none"
          :to="{
            name: 'Browse',
            params: { id: repository.id, catchAll: '' },
          }">
          Browse
        </v-btn>
      </v-card-text>
    </v-card>

    <v-row class="repository-page__content" align="stretch" no-gutters>
      <v-col cols="12" lg="8" class="pr-lg-6 mb-6 mb-lg-0">
        <RepositoryPageViewer
          v-if="repositoryPage"
          :repository="repository"
          :page="repositoryPage" />
      </v-col>
      <v-col cols="12" lg="4">
        <RepositoryHelper :repository="repository" />
      </v-col>
    </v-row>
  </v-container>
  <ErrorOnRequest
    v-else-if="error"
    :error="error"
    :errorCode="errorCode" />
</template>

<script setup lang="ts">
import CopyURL from "@/components/core/code/CopyCode.vue";
import ErrorOnRequest from "@/components/ErrorOnRequest.vue";
import RepositoryHelper from "@/components/nr/repository/RepositoryHelper.vue";
import RepositoryIcon from "@/components/nr/repository/RepositoryIcon.vue";
import RepositoryPageViewer from "@/components/nr/repository/RepositoryPageViewer.vue";
import { computed, onMounted, ref } from "vue";
import http from "@/http";
import router from "@/router";
import { useRepositoryStore } from "@/stores/repositories";
import {
  createRepositoryRoute,
  findRepositoryType,
  type RepositoryPage,
  type RepositoryWithStorageName,
} from "@/types/repository";

const repoStore = useRepositoryStore();

const repositoryId = ref<string | undefined>(undefined);
const repository = ref<RepositoryWithStorageName | undefined>(undefined);
const repositoryPage = ref<RepositoryPage | undefined>(undefined);
const error = ref<string | null>(null);
const errorCode = ref<number | undefined>(undefined);

const repositoryType = computed(() => {
  if (repository.value) {
    return findRepositoryType(repository.value.repository_type);
  }
  return undefined;
});

const url = computed(() => {
  if (!repository.value) {
    return "";
  }
  return createRepositoryRoute(repository.value);
});

function isPageUnsupported(err: unknown): boolean {
  const status = (err as any)?.response?.status;
  if (status !== 404 && status !== 400) {
    return false;
  }
  const message: string | undefined = (err as any)?.response?.data;
  return typeof message === "string" && message.includes("does not support config key page");
}

async function fetchRepository() {
  if (!repositoryId.value) {
    error.value = "Repository not found";
    return;
  }

  try {
    repository.value = await repoStore.getRepositoryById(repositoryId.value);
  } catch (err) {
    error.value = "Failed to load repository details.";
    return;
  }

  try {
    const response = await http.get<RepositoryPage>(`/api/repository/page/${repositoryId.value}`);
    repositoryPage.value = response.data;
    error.value = null;
    errorCode.value = undefined;
  } catch (err) {
    if (isPageUnsupported(err)) {
      repositoryPage.value = undefined;
      return;
    }

    console.error("Failed to load repository page", err);
    errorCode.value = (err as any)?.response?.status;
    error.value = "Failed to fetch repository";
  }
}

onMounted(() => {
  const { repositoryId: repoIdParam, storageName, repositoryName } = router.currentRoute.value.params;
  if (typeof repoIdParam === "string") {
    repositoryId.value = repoIdParam;
    fetchRepository();
    return;
  }

  if (typeof storageName === "string" && typeof repositoryName === "string") {
    repoStore
      .getRepositoryIdByNames(storageName, repositoryName)
      .then((response) => {
        if (!response) {
          error.value = "Repository not found";
          return;
        }
        repositoryId.value = response;
        fetchRepository();
      })
      .catch(() => {
        error.value = "Repository lookup failed.";
      });
  } else {
    error.value = "Repository not found";
  }
});
</script>
<style scoped lang="scss">
.repository-page {
  padding-top: 1.5rem;
  padding-bottom: 2rem;
}

.repository-page__header {
  border-radius: 16px;
}

.repository-page__header-content {
  display: flex;
  flex-direction: column;
  gap: 1rem;

  @media (min-width: 960px) {
    flex-direction: row;
    align-items: center;
    justify-content: space-between;
  }
}

.repository-page__meta {
  display: flex;
  align-items: center;
  gap: 1rem;
  flex-wrap: wrap;
}

.repository-page__icons {
  display: flex;
  gap: 0.5rem;
}

.repository-page__content {
  margin-top: 1.5rem;
}
</style>
