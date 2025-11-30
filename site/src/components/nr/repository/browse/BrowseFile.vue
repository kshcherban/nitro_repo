<template>
  <tr
    class="browse__row browse__row--file"
    data-type="file"
    role="button"
    tabindex="0"
    @click="activate"
    @keyup.enter.prevent="activate"
    @keyup.space.prevent="activate">
    <td class="browse__cell browse__cell--name">
      <div class="browse__cell-content">
        <font-awesome-icon :icon="fileIcon" />
        <span class="browse__name">{{ props.file.name }}</span>
      </div>
    </td>
    <td class="browse__cell browse__cell--meta">
      {{ formattedModified }}
    </td>
  </tr>
</template>

<script setup lang="ts">
import { fixCurrentPath, type RawFile } from "@/types/browse";
import { createRepositoryRoute, type RepositoryWithStorageName } from "@/types/repository";
import { computed, type PropType } from "vue";
import "./browse.scss";
const props = defineProps({
  file: {
    type: Object as PropType<RawFile>,
    required: true,
  },
  currentPath: {
    type: String,
    required: true,
  },
  repository: {
    type: Object as PropType<RepositoryWithStorageName>,
    required: true,
  },
});

const fixedPath = fixCurrentPath(props.currentPath);
const repositoryURL = createRepositoryRoute(props.repository, `${fixedPath}/${props.file.name}`);

const fileIcon = computed(() => "fa-solid fa-file" /* TODO: file-type specific */);

const formattedModified = computed(() =>
  new Date(props.file.modified).toLocaleString(),
);

function activate() {
  window.open(repositoryURL, "_blank");
}
</script>
