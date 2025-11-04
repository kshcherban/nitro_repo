<template>
  <main v-if="repository">
    <TabsElement>
      <template #header>
        <TabElement id="main"> Main </TabElement>
        <TabElement id="packages" v-if="showPackagesTab"> Packages </TabElement>
        <TabElement
          :id="configType"
          v-for="configType in configTypes"
          :key="configType">
          {{ getConfigTitleOrFallback(configType) }}
        </TabElement>
      </template>
      <template #content>
        <TabContent tabId="main">
          <BasicRepositoryInfo :repository="repository" />
        </TabContent>
        <TabContent v-if="showPackagesTab" tabId="packages">
          <RepositoryPackagesTab
            :repository-id="repositoryId"
            :repository-type="repository?.repository_type"
            :repository-kind="repositoryKind" />
        </TabContent>
        <TabContent
          class="tab-content"
          v-for="configType in configComponents"
          :tabId="configType.configName"
          :key="configType.configName">
          <component
            class="config"
            :is="configType.component"
            v-bind="configType.props" />
        </TabContent>
      </template>
    </TabsElement>
  </main>
</template>
<script setup lang="ts">
import BasicRepositoryInfo from "@/components/admin/repository/BasicRepositoryInfo.vue";
import FallBackEditor from "@/components/admin/repository/configs/FallBackEditor.vue";
import RepositoryPackagesTab from "@/components/admin/repository/RepositoryPackagesTab.vue";
import TabContent from "@/components/core/tabs/TabContent.vue";
import TabElement from "@/components/core/tabs/TabElement.vue";
import TabsElement from "@/components/core/tabs/TabsElement.vue";
import http from "@/http";
import router from "@/router";
import { useRepositoryStore } from "@/stores/repositories";
import {
  getConfigType,
  type ConfigDescription,
  type RepositoryWithStorageName,
} from "@/types/repository";
import { computed, ref, watch } from "vue";
const repositoryTypesStore = useRepositoryStore();
const repositoryId = router.currentRoute.value.params.id as string;

const repository = ref<RepositoryWithStorageName | undefined>(undefined);
const configDescriptions = ref<Map<string, ConfigDescription>>(new Map());
const configTypes = ref<string[]>([]);
const repositoryKind = ref<string | null>(null);
const showPackagesTab = computed(() => {
  const type = repository.value?.repository_type;
  return type === "python" || type === "npm" || type === "maven";
});
function getConfigTitleOrFallback(config: string) {
  return configDescriptions.value.get(config)?.name || config;
}
watch(configTypes, async () => {
  for (const config of configTypes.value) {
    await repositoryTypesStore.getConfigDescription(config).then((response) => {
      if (response) {
        configDescriptions.value.set(config, response);
      }
    });
  }
});
const configComponents = computed(() => {
  const configs = configTypes.value.map((config) => {
    const component = getConfigType(config);
    if (component) {
      return {
        component: component.component,
        configName: config,
        props: {
          repository: repositoryId,
        },
      };
    } else {
      return {
        component: FallBackEditor,
        configName: config,
        props: {
          settingName: config,
          repository: repositoryId,
        },
      };
    }
  });
  console.log(configs);
  return configs;
});

async function getRepository() {
  await http
    .get(`/api/repository/${repositoryId}`, {
      params: { include_usage: true },
    })
    .then((response) => {
      repository.value = response.data;
    });
  await http.get(`/api/repository/${repositoryId}/configs`).then((response) => {
    configTypes.value = response.data;
  });
  await loadRepositoryKind();
}
getRepository();

async function loadRepositoryKind() {
  const type = repository.value?.repository_type;
  if (!type) {
    repositoryKind.value = null;
    return;
  }
  const configKey = type.toLowerCase();
  try {
    const response = await http.get(`/api/repository/${repositoryId}/config/${configKey}`);
    if (response?.data?.type) {
      repositoryKind.value = String(response.data.type);
    } else {
      repositoryKind.value = null;
    }
  } catch (error) {
    console.error("Failed to load repository kind", error);
    repositoryKind.value = null;
  }
}
</script>
<style scoped lang="scss">
@import "@/assets/styles/theme";
</style>
