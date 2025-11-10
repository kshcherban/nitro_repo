import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/http", () => ({
  default: {
    get: vi.fn(),
  },
}));

import PhpConfig from "../PhpConfig.vue";
import http from "@/http";

const textInputStub = {
  template: `<label class="stub-text-input">
    <slot />
    <input :value="modelValue" disabled />
  </label>`,
  props: {
    modelValue: {
      type: [String, Number],
      default: "",
    },
  },
};

describe("PhpConfig.vue", () => {
  it("renders hosted type information", () => {
    const wrapper = mount(PhpConfig, {
      props: {
        modelValue: { type: "Hosted" },
        "onUpdate:modelValue": () => {},
      },
      global: {
        stubs: {
          TextInput: textInputStub,
        },
      },
    });

    expect(wrapper.find('[data-testid="php-config-card"]').exists()).toBe(true);
  });

  it("loads existing configuration when repository is provided", async () => {
    (http.get as vi.Mock).mockResolvedValueOnce({
      data: { type: "Hosted" },
    });

    mount(PhpConfig, {
      props: {
        repository: "repo-1",
        modelValue: { type: "Hosted" },
        "onUpdate:modelValue": () => {},
      },
      global: {
        stubs: {
          TextInput: textInputStub,
        },
      },
    });

    await flushPromises();
    expect(http.get).toHaveBeenCalledWith("/api/repository/repo-1/config/php");
  });
});
