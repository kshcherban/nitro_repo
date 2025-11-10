<template>
  <div
    v-if="form && model"
    class="editorBox">
    <h2 class="settingHeader">Generic Config Editor: {{ settingName }}</h2>

    <JsonSchemaForm
      :form="form"
      v-model="model" />
    <button
      type="submit"
      class="nr-button nr-button--primary"
      @click="save">
      Save Configuration
    </button>
  </div>
</template>
<script setup lang="ts">
import http from "@/http";
import { computed, ref, type PropType } from "vue";
import { useRepositoryStore } from "@/stores/repositories";
import JsonSchemaForm from "@/components/form/JsonSchemaForm.vue";
import { createForm, type RootSchema } from "nitro-jsf";

const schema = ref<RootSchema | undefined>(undefined);
const form = computed(() => {
  if (schema.value) {
    return createForm(schema.value);
  }
  return undefined;
});
const props = defineProps({
  settingName: String,
  repository: {
    type: Object as PropType<string>,
    required: false,
  },
});
const model = defineModel<any>();
const repositoryTypeStore = useRepositoryStore();
async function load() {
  if (!props.settingName) {
    throw new Error("settingName is required");
  }
  await repositoryTypeStore.getConfigSchema(props.settingName).then((response) => {
    schema.value = response as RootSchema;
    console.log(schema.value);
  });
  if (props.repository) {
    await http
      .get(`/api/repository/${props.repository}/config/${props.settingName}?default=true`)
      .then((response) => {
        model.value = response.data;
      })
      .catch((error) => {
        console.error(error);
      });
    if (!model.value) {
      loadDefault();
    }
  } else {
    loadDefault();
  }
}
async function loadDefault() {
  await http
    .get(`/api/repository/repository/${props.repository}/config/${props.settingName}`)
    .then((response) => {
      model.value = response.data;
    })
    .catch((error) => {
      console.error(error);
    });
}

async function save() {
  if (!props.repository || !props.settingName || !model.value) {
    console.error("Missing required properties for save");
    return;
  }
  try {
    await http.put(
      `/api/repository/${props.repository}/config/${props.settingName}`,
      model.value
    );
    await load();
  } catch (error) {
    console.error("Failed to save configuration", error);
  }
}

load();
</script>
<style scoped lang="scss">
@import "@/assets/styles/buttons.scss";

.editorBox {
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
}

.settingHeader {
  margin: 0;
  font-size: 1.25rem;
  font-weight: 500;
}

.nr-button {
  align-self: flex-start;
  min-width: 10rem;
}
</style>
