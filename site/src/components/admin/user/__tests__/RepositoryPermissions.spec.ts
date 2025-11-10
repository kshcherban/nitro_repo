import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { defineComponent } from "vue";
import RepositoryPermissions from "@/components/admin/user/RepositoryPermissions.vue";

vi.mock("@vue/devtools-kit", () => ({}));

vi.mock("@/http", () => ({
  default: {
    get: vi.fn().mockResolvedValue({
      data: {
        admin: false,
        user_manager: false,
        storage_manager: false,
        repository_manager: false,
        default_repository_actions: [],
        repository_permissions: {
          repo1: ["read", "write"],
        },
      },
    }),
    put: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("@/stores/repositories", () => ({
  useRepositoryStore: () => ({
    getRepositoryById: vi.fn().mockImplementation(async (id: string) => {
      if (id === "repo1") {
        return { id: "repo1", name: "Repository One" };
      }
      if (id === "repo2") {
        return { id: "repo2", name: "Repository Two" };
      }
      return null;
    }),
  }),
}));

const SubmitButtonStub = defineComponent({
  props: {
    block: {
      type: Boolean,
      default: true,
    },
    disabled: Boolean,
  },
  emits: ["click"],
  template: `<button class="submit-button-stub" :data-block="block" :disabled="disabled" @click="$emit('click')"><slot /></button>`,
});

const VSwitchStub = defineComponent({
  props: {
    modelValue: Boolean,
    color: String,
  },
  emits: ["update:modelValue"],
  template: `
    <label class="v-switch-stub" :data-color="color">
      <input type="checkbox" :checked="modelValue" @change="$emit('update:modelValue', $event.target.checked)" />
    </label>
  `,
});

const VBtnStub = defineComponent({
  emits: ["click"],
  template: `<button class="v-btn-stub" @click="$emit('click')"><slot /></button>`,
});

const RepositoryDropdownStub = defineComponent({
  props: {
    modelValue: {
      type: String,
      default: "",
    },
  },
  emits: ["update:modelValue"],
  template: `
    <select
      class="repository-dropdown-stub"
      :value="modelValue"
      @change="$emit('update:modelValue', $event.target.value)">
      <option value="">Select</option>
      <option value="repo2">Repository Two</option>
    </select>
  `,
});

const defaultUser = {
  id: 1,
  name: "User",
  username: "user",
  email: "user@example.com",
  default_repository_actions: [],
};

describe("RepositoryPermissions.vue", () => {
  it("renders switches with primary color and save button left-aligned", async () => {
    const wrapper = mount(RepositoryPermissions, {
      props: {
        user: defaultUser as any,
      },
      global: {
        stubs: {
          SubmitButton: SubmitButtonStub,
          "v-switch": VSwitchStub,
          "v-btn": VBtnStub,
          RepositoryDropdown: RepositoryDropdownStub,
        },
        directives: {
          "auto-animate": () => undefined,
        },
      },
    });

    await flushPromises();

    const switches = wrapper.findAllComponents(VSwitchStub);
    expect(switches.length).toBeGreaterThan(0);
    for (const toggle of switches) {
      expect(toggle.attributes("data-color")).toBe("primary");
    }

    const footer = wrapper.find(".repository-permissions__footer");
    expect(footer.exists()).toBe(true);
    const button = footer.findComponent(SubmitButtonStub);
    expect(button.exists()).toBe(true);
    expect(button.props("block")).toBe(false);
  });
});
