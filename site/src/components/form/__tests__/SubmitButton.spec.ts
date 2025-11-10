import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import SubmitButton from "@/components/form/SubmitButton.vue";

const vuetifyStubs = {
  "v-btn": defineComponent({
    props: {
      type: {
        type: String,
        default: "button",
      },
      color: String,
      variant: String,
      block: Boolean,
      loading: Boolean,
      disabled: Boolean,
    },
    emits: ["click"],
    template: `
      <button
        class="v-btn"
        :type="type"
        :disabled="disabled"
        @click="$emit('click', $event)">
        <slot />
      </button>
    `,
  }),
};

describe("SubmitButton.vue", () => {
  it("renders as a Vuetify button with submit semantics", () => {
    const wrapper = mount(SubmitButton, {
      slots: { default: "Create" },
      global: {
        stubs: vuetifyStubs,
      },
    });

    const button = wrapper.get("button");
    expect(button.classes()).toContain("v-btn");
    expect(button.attributes("type")).toBe("submit");
    expect(button.text()).toBe("Create");
  });

  it("emits a click event when activated", async () => {
    const wrapper = mount(SubmitButton, {
      global: {
        stubs: vuetifyStubs,
      },
    });

    await wrapper.get("button").trigger("click");

    expect(wrapper.emitted("click")).toHaveLength(1);
  });
});
