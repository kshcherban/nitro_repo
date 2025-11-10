import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import { reactive } from "vue";

import MavenProxyConfig from "../MavenProxyConfig.vue";

const textInputStub = {
  template: `<label class="stub-text-field">
    <slot />
    <input :value="modelValue" @input="$emit('update:modelValue', $event.target.value)" />
  </label>`,
  props: {
    modelValue: {
      type: [String, Number],
      default: "",
    },
  },
};

describe("MavenProxyConfig.vue", () => {
  it("removes routes when remove is clicked", async () => {
    const model = reactive({
      routes: [
        { url: "https://repo1.example.com", name: "Primary" },
        { url: "https://repo2.example.com", name: "Secondary" },
      ],
    });

    const wrapper = mount(MavenProxyConfig, {
      props: {
        modelValue: model,
        "onUpdate:modelValue": (val: typeof model) => {
          model.routes = val.routes;
        },
      },
      global: {
        stubs: {
          TextInput: textInputStub,
        },
      },
    });

    await wrapper.findAll("button").at(0)?.trigger("click");
    expect(model.routes).toHaveLength(1);
  });

  it("adds a new route when provided URL is valid", async () => {
    const model = reactive({
      routes: [{ url: "https://repo1.example.com", name: "Primary" }],
    });

    const wrapper = mount(MavenProxyConfig, {
      props: {
        modelValue: model,
        "onUpdate:modelValue": (val: typeof model) => {
          model.routes = val.routes;
        },
      },
      global: {
        stubs: {
          TextInput: textInputStub,
        },
      },
    });

    const inputs = wrapper.findAll("input");
    const urlInput = inputs.at(inputs.length - 2);
    const nameInput = inputs.at(inputs.length - 1);

    await urlInput?.setValue("https://repo3.example.com");
    await nameInput?.setValue("Tertiary");
    await wrapper.findAll("button").at(-1)?.trigger("click");

    expect(model.routes).toHaveLength(2);
    expect(model.routes[1]).toEqual({
      url: "https://repo3.example.com",
      name: "Tertiary",
    });
  });
});
