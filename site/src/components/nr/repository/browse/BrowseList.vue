<template>
  <div id="browseList">
    <table class="browse__table">
      <thead>
        <tr>
          <th
            class="browse__header-cell browse__header-cell--name"
            data-column="name">
            Name
          </th>
          <th
            class="browse__header-cell browse__header-cell--meta"
            data-column="details">
            Details
          </th>
        </tr>
      </thead>
      <tbody>
        <BrowseEntry
          v-for="file in sortedFiles"
          :key="file.value.name"
          :file="file"
          :currentPath="currentPath"
          :repository="repository" />
        <SkeletonEntry
          v-for="i in skeletons"
          :key="i" />
      </tbody>
    </table>
  </div>
</template>
<script setup lang="ts">
import { type RawBrowseFile } from "@/types/browse";
import type { RepositoryWithStorageName } from "@/types/repository";

import BrowseEntry from "./BrowseEntry.vue";
import { computed, nextTick, type PropType, watch } from "vue";
import SkeletonEntry from "./SkeletonEntry.vue";
import { useResizableColumns } from "@/composables/useResizableColumns";

const props = defineProps({
  files: {
    type: Array as PropType<RawBrowseFile[]>,
    required: true,
  },
  totalFiles: {
    type: Number,
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
const skeletons = computed(() => {
  const skeletonsArray = [];
  for (let i = props.files.length; i < props.totalFiles; i++) {
    skeletonsArray.push(i);
  }
  return skeletonsArray;
});
const sortedFiles = computed(() => {
  const files = props.files;
  return files.sort((a, b) => {
    if (a.type === "Directory" && b.type === "File") {
      return -1;
    } else if (a.type === "File" && b.type === "Directory") {
      return 1;
    } else {
      return a.value.name.localeCompare(b.value.name);
    }
  });
});

const { initResizable: initBrowseResizers } = useResizableColumns("#browseList .browse__table");

watch(
  () => sortedFiles.value.length,
  (length) => {
    if (length === 0) {
      return;
    }
    nextTick(() => {
      initBrowseResizers();
    });
  },
  { flush: "post" },
);
</script>
<style lang="scss" scoped>
@use "@/assets/styles/theme.scss" as *;
#browseList {
  padding: 1rem;
}

.browse__table {
  width: 100%;
  border-collapse: collapse;
  table-layout: fixed;
  background: var(--nr-background-primary, #fff);
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 8px;
  overflow: hidden;
}

.browse__header-cell {
  text-align: left;
  font-weight: 600;
  font-size: 0.9rem;
  padding: 0.75rem 1rem;
  background: var(--nr-background-tertiary, #f8f9fa);
  border-bottom: 1px solid rgba(0, 0, 0, 0.08);
  position: relative;
}

.browse__header-cell--meta {
  text-align: right;
}
</style>
