<template>
  <v-btn
    class="submit-button submit-button--fixed"
    :type="type"
    :block="block"
    :loading="loading"
    :disabled="disabled"
  :color="color"
    :variant="variant"
    v-bind="$attrs"
    @click="handleClick">
    <slot />
  </v-btn>
</template>
<script setup lang="ts">
import { toRefs } from "vue";

const emit = defineEmits<{
  (e: "click", event: MouseEvent): void;
}>();

const props = withDefaults(
  defineProps<{
    block?: boolean;
    loading?: boolean;
    disabled?: boolean;
    color?: string;
    variant?: "flat" | "outlined" | "text" | "tonal" | "elevated";
    type?: "submit" | "button" | "reset";
  }>(),
  {
    block: true,
    loading: false,
    disabled: false,
    color: "primary",
    variant: "flat",
    type: "submit",
  },
);

const { block, loading, disabled, color, variant, type } = toRefs(props);

function handleClick(event: MouseEvent) {
  emit("click", event);
}
</script>

<style scoped lang="scss">
.submit-button {
  text-transform: none;
  font-weight: 600;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 0.5rem;
  padding: 0.5rem 1.25rem;
}

.submit-button--fixed {
  min-width: 6.25rem;
}
</style>
