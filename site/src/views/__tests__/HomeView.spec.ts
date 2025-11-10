import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { defineComponent, nextTick } from "vue";
import HomeView from "@/views/HomeView.vue";

vi.mock("vue-router", () => ({
  useRouter: () => ({
    push: vi.fn(),
  }),
}));

const repositoriesMock = vi.fn().mockResolvedValue([
  {
    id: 1,
    name: "Alpha",
    repository_type: "npm",
    storage_name: "Primary",
    auth_enabled: true,
    storage_usage_bytes: 0,
    active: true,
  },
]);

vi.mock("@/stores/repositories", () => ({
  useRepositoryStore: () => ({
    getRepositories: repositoriesMock,
  }),
}));

vi.mock("@/stores/session", () => ({
  sessionStore: () => ({
    user: { admin: false },
  }),
}));

const VContainerStub = defineComponent({
  template: "<div class='v-container'><slot /></div>",
});

const VRowStub = defineComponent({
  template: "<div class='v-row'><slot /></div>",
});

const VColStub = defineComponent({
  template: "<div class='v-col'><slot /></div>",
});

const VAvatarStub = defineComponent({
  template: "<div class='v-avatar'><slot /></div>",
});

const VBtnStub = defineComponent({
  props: {
    to: [String, Object],
  },
  template: "<button class='v-btn'><slot /></button>",
});

const VCardStub = defineComponent({
  template: "<div class='v-card'><slot /></div>",
});

const VCardTitleStub = defineComponent({
  template: "<div class='v-card-title'><slot /></div>",
});

const VCardTextStub = defineComponent({
  template: "<div class='v-card-text'><slot /></div>",
});

const VCardActionsStub = defineComponent({
  template: "<div class='v-card-actions'><slot /></div>",
});

const VChipStub = defineComponent({
  template: "<span class='v-chip'><slot /></span>",
});

const VIconStub = defineComponent({
  template: "<i class='v-icon'><slot /></i>",
});

const VProgressCircularStub = defineComponent({
  template: "<div class='v-progress-circular'><slot /></div>",
});

const VAlertStub = defineComponent({
  template: "<div class='v-alert'><slot /></div>",
});

const VTextFieldStub = defineComponent({
  props: {
    modelValue: {
      type: String,
      default: "",
    },
    clearable: {
      type: Boolean,
      default: false,
    },
  },
  emits: ["update:modelValue", "click:clear"],
  template: `
    <label class="v-text-field">
      <input
        :value="modelValue"
        @input="$emit('update:modelValue', $event.target.value)" />
      <button
        type="button"
        class="v-text-field__clear"
        @click="$emit('click:clear')">
        clear
      </button>
      <slot />
    </label>
  `,
});

const vuetifyStubs = {
  "v-container": VContainerStub,
  "v-row": VRowStub,
  "v-col": VColStub,
  "v-avatar": VAvatarStub,
  "v-btn": VBtnStub,
  "v-card": VCardStub,
  "v-card-title": VCardTitleStub,
  "v-card-text": VCardTextStub,
  "v-card-actions": VCardActionsStub,
  "v-chip": VChipStub,
  "v-icon": VIconStub,
  "v-progress-circular": VProgressCircularStub,
  "v-alert": VAlertStub,
  "v-text-field": VTextFieldStub,
};

describe("HomeView.vue", () => {
  it("provides a clearable repository search input", async () => {
    const wrapper = mount(HomeView, {
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    const field = wrapper.getComponent(VTextFieldStub);
    expect(field.props("clearable")).toBe(true);

    field.vm.$emit("update:modelValue", "alp");
    await nextTick();
    expect(wrapper.vm.searchTerm).toBe("alp");

    field.vm.$emit("click:clear");
    await nextTick();
    expect(wrapper.vm.searchTerm).toBe("");
  });
});
