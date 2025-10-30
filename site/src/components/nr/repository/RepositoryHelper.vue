<template>
  <div v-if="repositoryHelper">
    <component
      :is="repositoryHelper.component"
      :repository="repository" />
  </div>
  <div v-else>
    <p>Repository type not supported</p>
  </div>
</template>

<script setup lang="ts">
import type { RepositoryWithStorageName } from "@/types/repository";
import { computed, type PropType } from "vue";
import MavenRepositoryHelper from "./types/maven/MavenRepositoryHelper.vue";
import PythonRepositoryHelper from "./types/python/PythonRepositoryHelper.vue";
import PhpRepositoryHelper from "./types/php/PhpRepositoryHelper.vue";

const props = defineProps({
  repository: {
    type: Object as PropType<RepositoryWithStorageName>,
    required: true,
  },
});
const helpers = [
  {
    type: "maven",
    component: MavenRepositoryHelper,
  },
  {
    type: "python",
    component: PythonRepositoryHelper,
  },
  {
    type: "php",
    component: PhpRepositoryHelper,
  },
];
const repositoryHelper = computed(() => {
  return helpers.find((helper) => helper.type === props.repository.repository_type);
});
</script>
