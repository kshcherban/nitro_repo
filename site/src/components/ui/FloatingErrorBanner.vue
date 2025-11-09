<template>
  <transition name="floating-error-fade">
    <div
      v-if="visible"
      class="floating-error"
      role="alertdialog"
      aria-live="assertive"
      aria-modal="false">
      <h3 class="floating-error__title">{{ title }}</h3>
      <p class="floating-error__message">{{ message }}</p>
      <div class="floating-error__actions">
        <button type="button" @click="$emit('close')" aria-label="Dismiss error">
          Dismiss
        </button>
      </div>
    </div>
  </transition>
</template>

<script setup lang="ts">
defineProps({
  visible: {
    type: Boolean,
    default: false,
  },
  title: {
    type: String,
    default: "",
  },
  message: {
    type: String,
    default: "",
  },
});

defineEmits<{
  (e: "close"): void;
}>();
</script>

<style scoped lang="scss">
@import "@/assets/styles/theme.scss";

.floating-error {
  position: fixed;
  top: 25%;
  left: 50%;
  transform: translateX(-50%);
  max-width: 420px;
  width: calc(100% - 2rem);
  background-color: $background-90;
  border-radius: 1rem;
  box-shadow: 0 18px 36px rgba(0, 0, 0, 0.45);
  padding: 1.5rem;
  display: flex;
  flex-direction: column;
  gap: 1rem;
  border: 1px solid $secondary-70;
  z-index: 1200;
}

.floating-error__title {
  font-size: 1.1rem;
  font-weight: 600;
  color: $text;
}

.floating-error__message {
  color: $text-50;
  line-height: 1.5;
  white-space: pre-wrap;
}

.floating-error__actions {
  display: flex;
  justify-content: flex-end;
}

.floating-error__actions button {
  background: transparent;
  border: none;
  color: $primary;
  font-weight: 600;
  cursor: pointer;
  padding: 0.25rem 0.75rem;
  border-radius: 0.5rem;
}

.floating-error__actions button:hover,
.floating-error__actions button:focus-visible {
  background-color: $primary-30;
}

.floating-error-fade-enter-active,
.floating-error-fade-leave-active {
  transition: opacity 0.2s ease;
}

.floating-error-fade-enter-from,
.floating-error-fade-leave-to {
  opacity: 0;
}
</style>
