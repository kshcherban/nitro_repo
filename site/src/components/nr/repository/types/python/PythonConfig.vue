<template>
  <form class="python-config" @submit.prevent="save">
    <DropDown
      v-model="selectedType"
      :options="typeOptions"
      :disabled="!isCreate"
      required
    >Repository Type</DropDown>

    <div v-if="isProxy" class="proxy-routes">
      <div
        v-for="(route, index) in proxyRoutes"
        :key="index"
        class="route-row"
      >
        <TextInput v-model="route.url" required placeholder="https://pypi.org/simple"
          >Upstream URL</TextInput
        >
        <TextInput v-model="route.name" placeholder="Optional label">Display Name</TextInput>
        <button
          class="nr-button nr-button--danger"
          type="button"
          @click="removeRoute(index)"
        >Remove</button>
      </div>
      <button class="nr-button" type="button" @click="addRoute">Add Route</button>
    </div>

    <button
      v-if="!isCreate"
      class="nr-button nr-button--primary"
      type="submit"
    >Save</button>
  </form>
</template>
<script setup lang="ts">
import { computed, defineProps, onMounted, ref, watch } from "vue";
import DropDown from "@/components/form/dropdown/DropDown.vue";
import TextInput from "@/components/form/text/TextInput.vue";
import http from "@/http";
import { defaultProxy, type PythonConfigType } from "./python";

const typeOptions = [
  { value: "Hosted", label: "Hosted" },
  { value: "Proxy", label: "Proxy" },
];

const props = defineProps({
  settingName: String,
  repository: {
    type: String,
    required: false,
  },
});

const value = defineModel<PythonConfigType>({
  default: { type: "Hosted" },
});

const selectedType = ref<string>(value.value?.type ?? "Hosted");
const isCreate = computed(() => !props.repository);
const isProxy = computed(() => value.value?.type === "Proxy");
const proxyRoutes = computed(() => {
  if (value.value?.type !== "Proxy") {
    return [] as ReturnType<typeof defaultProxy>["routes"];
  }
  return value.value.config.routes;
});

watch(selectedType, (newType) => {
  if (newType === "Proxy") {
    if (value.value?.type !== "Proxy") {
      value.value = {
        type: "Proxy",
        config: defaultProxy(),
      };
    }
  } else {
    value.value = { type: "Hosted" };
  }
});

function ensureProxyConfig() {
  if (value.value?.type !== "Proxy") {
    value.value = {
      type: "Proxy",
      config: defaultProxy(),
    };
  }
}

function addRoute() {
  ensureProxyConfig();
  if (value.value?.type === "Proxy") {
    value.value.config.routes.push({ url: "", name: "" });
  }
}

function removeRoute(index: number) {
  if (value.value?.type === "Proxy") {
    value.value.config.routes.splice(index, 1);
  }
}

async function load() {
  if (!props.repository) {
    return;
  }
  try {
    const response = await http.get(`/api/repository/${props.repository}/config/python`);
    value.value = response.data;
    selectedType.value = value.value?.type ?? "Hosted";
  } catch (error) {
    console.error(error);
  }
}

async function save() {
  if (!props.repository) {
    return;
  }
  try {
    await http.put(`/api/repository/${props.repository}/config/python`, value.value);
  } catch (error) {
    console.error(error);
  }
}

onMounted(() => {
  if (!value.value) {
    value.value = { type: "Hosted" };
  }
  load();
});
</script>

<style scoped>
.python-config {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.proxy-routes {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}
.route-row {
  display: grid;
  gap: 0.5rem;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  align-items: end;
}
.nr-button {
  width: fit-content;
  padding: 0.4rem 0.8rem;
}
.nr-button--primary {
  background: var(--brand-color, #2d7ef7);
  color: #fff;
  border: none;
  border-radius: 4px;
}
.nr-button--danger {
  background: #d9534f;
  color: #fff;
  border: none;
  border-radius: 4px;
}
</style>
