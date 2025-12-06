<template>
  <div class="switch-wrapper">
    <v-switch
      :id="id"
      v-model="value"
      :aria-label="ariaLabel"
      color="primary"
      hide-details>
      <template #label>
        <div
          v-if="!hideLabel"
          class="switch-label-content">
          <span class="switch-label-text">
            <slot />
          </span>
          <span v-if="$slots.comment" class="switch-comment">
            <slot name="comment" />
          </span>
        </div>
      </template>
    </v-switch>
  </div>
</template>
<script setup lang="ts">
import { watch } from "vue";

defineProps({
  id: {
    type: String,
    required: true,
  },
  hideLabel: {
    type: Boolean,
    default: false,
  },
  ariaLabel: {
    type: String,
    default: undefined,
  },
});

const value = defineModel<boolean>({
  required: true,
});

const emit = defineEmits<{
  (e: "change", newValue: boolean): void;
}>();

watch(value, (newValue) => {
  emit("change", newValue);
});
</script>

<style scoped lang="scss">
.switch-wrapper {
  margin: 1rem 0;
}

.switch-label-content {
  display: flex;
  flex-direction: column;
}

.switch-label-text {
  font-size: 1rem;
  font-weight: 500;
  color: var(--nr-text-primary);
}

.switch-comment {
  font-size: 0.875rem;
  color: var(--nr-text-secondary);
  margin-top: 0.25rem;
}
</style>
