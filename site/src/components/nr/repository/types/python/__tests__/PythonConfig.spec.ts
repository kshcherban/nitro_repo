import { flushPromises, mount } from "@vue/test-utils";
import { beforeAll, describe, expect, it } from "vitest";
import { defineComponent, h, ref } from "vue";

const storageMock = {
  getItem: () => null,
  setItem: () => undefined,
  removeItem: () => undefined,
  clear: () => undefined,
};

(globalThis as any).localStorage = storageMock;
if (typeof window !== "undefined") {
  (window as any).localStorage = storageMock;
}

let PythonConfig: any;

beforeAll(async () => {
  PythonConfig = (await import("../PythonConfig.vue")).default;
});

const DropDownStub = defineComponent({
  props: ["modelValue", "options"],
  emits: ["update:modelValue"],
  setup(props, { emit, slots }) {
    const local = ref(props.modelValue ?? "");
    const onChange = (event: Event) => {
      const next = (event.target as HTMLSelectElement).value;
      local.value = next;
      emit("update:modelValue", next);
    };
    return { local, slots, onChange };
  },
  template: `
    <label class="dropdown-stub">
      <slot />
      <select :value="local" @change="onChange">
        <option v-for="option in options" :key="option.value" :value="option.value">
          {{ option.label }}
        </option>
      </select>
    </label>
  `,
});

const TextInputStub = defineComponent({
  props: ["modelValue", "placeholder", "id"],
  emits: ["update:modelValue"],
  setup(props, { emit, slots }) {
    const onInput = (event: Event) => {
      const target = event.target as HTMLInputElement | null;
      emit("update:modelValue", target?.value ?? "");
    };
    return { props, slots, onInput };
  },
  template: `
    <label class="text-input-stub">
      <slot />
      <input
        :id="id"
        :placeholder="placeholder"
        :value="modelValue"
        @input="onInput" />
    </label>
  `,
});

const VBtnStub = defineComponent({
  inheritAttrs: false,
  emits: ["click"],
  setup(_, { emit, attrs, slots }) {
    return () =>
      h(
        "button",
        {
          ...attrs,
          type: (attrs.type as string) || "button",
          disabled: attrs.disabled as boolean | undefined,
          onClick: () => emit("click"),
        },
        slots.default?.(),
      );
  },
});

describe("PythonConfig proxy layout", () => {
  it("renders proxy remove buttons with the full-width action class", async () => {
    const wrapper = mount(PythonConfig, {
      props: { settingName: "python" },
      global: {
        stubs: {
          DropDown: DropDownStub,
          TextInput: TextInputStub,
          SubmitButton: defineComponent({ template: "<button type='submit'><slot /></button>" }),
          ProxyCacheNotice: defineComponent({ template: "<div />" }),
          "v-btn": VBtnStub,
        },
      },
    });

    await flushPromises();
    const typeSelect = wrapper.findComponent(DropDownStub);
    await typeSelect.find("select").setValue("Proxy");
    await flushPromises();

    const removeBtn = wrapper.get(".route-row button");
    expect(removeBtn.classes()).toContain("route-action");
  });
});
