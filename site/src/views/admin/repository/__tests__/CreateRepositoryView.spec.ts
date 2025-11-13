import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, ref } from "vue";

vi.mock("@vue/devtools-kit", () => ({}));

vi.mock("@/stores/repositories", () => ({
  useRepositoryStore: () => ({
    getStorages: vi.fn().mockResolvedValue([
      { id: "storage-1", name: "Primary", storage_type: "s3" },
    ]),
    getRepositoryTypes: vi.fn().mockResolvedValue([
      {
        type_name: "npm",
        name: "NPM",
        description: "Node packages",
        required_configs: [],
      },
    ]),
  }),
}));

vi.mock("@/http", () => ({
  default: {
    post: vi.fn(),
  },
}));

const mockAlerts = {
  success: vi.fn(),
  error: vi.fn(),
};

vi.mock("@/stores/alerts", () => ({
  useAlertsStore: () => mockAlerts,
}));

const controlStubs = {
  TextInput: defineComponent({
    props: ["modelValue", "id", "placeholder"],
    emits: ["update:modelValue"],
    template: `
      <label class="text-input">
        <slot />
        <input
          :id="id"
          :placeholder="placeholder"
          :value="modelValue"
          @input="$emit('update:modelValue', $event.target.value)" />
      </label>
    `,
  }),
  DropDown: defineComponent({
    props: ["modelValue", "options", "id"],
    emits: ["update:modelValue"],
    setup(props, { emit, slots }) {
      const value = ref(props.modelValue ?? "");
      const onChange = (event: Event) => {
        const next = (event.target as HTMLSelectElement).value;
        value.value = next;
        emit("update:modelValue", next);
      };
      return { props, slots, value, onChange };
    },
    template: `
      <label class="dropdown">
        <slot />
        <select :value="value" @change="onChange">
          <option value="" disabled>Select…</option>
          <option
            v-for="option in props.options"
            :key="option.value"
            :value="option.value">
            {{ option.label }}
          </option>
        </select>
      </label>
    `,
  }),
  SubmitButton: defineComponent({
    props: { disabled: Boolean, loading: Boolean },
    emits: ["click"],
    template: `
      <button
        class="v-btn"
        type="submit"
        :disabled="disabled"
        @click="$emit('click', $event)">
        <slot />
      </button>
    `,
  }),
  FloatingErrorBanner: defineComponent({
    props: ["visible", "title", "message"],
    emits: ["close"],
    template: `
      <div v-if="visible" data-testid="repository-create-error">
        <strong>{{ title }}</strong>
        <p>{{ message }}</p>
        <button type="button" @click="$emit('close')">Close</button>
      </div>
    `,
  }),
};

const vuetifyStubs = {
  "v-container": defineComponent({
    template: "<div data-testid='repository-create-container'><slot /></div>",
  }),
  "v-card": defineComponent({
    template: "<div data-testid='repository-create-card'><slot /></div>",
  }),
  "v-card-title": defineComponent({
    template: "<div class='v-card-title'><slot /></div>",
  }),
  "v-card-text": defineComponent({
    template: "<div class='v-card-text'><slot /></div>",
  }),
  "v-form": defineComponent({
    emits: ["submit"],
    template: "<form data-testid='repository-create-form'><slot /></form>",
  }),
  "v-row": defineComponent({
    template: "<div class='v-row'><slot /></div>",
  }),
  "v-col": defineComponent({
    props: { cols: [Number, String], md: [Number, String] },
    template: "<div class='v-col'><slot /></div>",
  }),
  "v-progress-circular": defineComponent({
    template: "<div data-testid='repository-create-loading'></div>",
  }),
  "v-alert": defineComponent({
    template: "<div data-testid='repository-create-alert'><slot /></div>",
  }),
  "v-divider": defineComponent({
    template: "<hr class='v-divider' />",
  }),
};

describe("CreateRepositoryView.vue", () => {
  let CreateRepositoryView: any;

  class LocalStorageMock implements Storage {
    private store = new Map<string, string>();

    get length(): number {
      return this.store.size;
    }

    clear(): void {
      this.store.clear();
    }

    getItem(key: string): string | null {
      return this.store.get(key) ?? null;
    }

    key(index: number): string | null {
      return Array.from(this.store.keys())[index] ?? null;
    }

    removeItem(key: string): void {
      this.store.delete(key);
    }

    setItem(key: string, value: string): void {
      this.store.set(key, value);
    }
  }

  const mockLocalStorage = new LocalStorageMock();

  beforeEach(async () => {
    mockAlerts.success.mockReset();
    mockAlerts.error.mockReset();
    Object.defineProperty(globalThis, "localStorage", {
      value: mockLocalStorage,
      configurable: true,
    });
    Object.defineProperty(window, "localStorage", {
      value: mockLocalStorage,
      configurable: true,
    });
    const module = await import("@/views/admin/repository/CreateRepositoryView.vue");
    CreateRepositoryView = module.default;
  });

  afterEach(() => {
    vi.resetModules();
  });

  it("renders the new card layout with a create action button", async () => {
    const wrapper = mount(CreateRepositoryView, {
      global: {
        stubs: {
          ...vuetifyStubs,
          ...controlStubs,
        },
      },
    });

    await flushPromises();

    expect(wrapper.find('[data-testid="repository-create-card"]').exists()).toBe(true);
    expect(wrapper.find(".v-btn").text()).toContain("Create");
  });
});
