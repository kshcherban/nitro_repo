import { flushPromises, mount } from "@vue/test-utils";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent } from "vue";

vi.mock("@/http", () => ({
  default: {
    get: vi.fn(),
  },
}));

vi.mock("@/router", () => ({
  default: {
    push: vi.fn(),
  },
}));

import PublicRepositoryList from "@/components/nr/repository/PublicRepositoryList.vue";
import http from "@/http";
import type { RepositoryWithStorageName } from "@/types/repository";

const repositories: RepositoryWithStorageName[] = [
  {
    id: "repo-1",
    storage_name: "primary",
    storage_id: "storage-1",
    name: "example-repo",
    repository_type: "npm",
    repository_kind: null,
    active: true,
    visibility: "Public",
    updated_at: "2025-11-09T00:00:00Z",
    created_at: "2025-01-01T00:00:00Z",
    auth_enabled: false,
    storage_usage_bytes: null,
    storage_usage_updated_at: null,
  },
];

const vuetifyStubs = {
  "v-text-field": defineComponent({
    props: {
      modelValue: {
        type: String,
        default: "",
      },
    },
    emits: ["update:modelValue"],
    setup(props, { emit, slots }) {
      const onInput = (event: Event) => {
        emit("update:modelValue", (event.target as HTMLInputElement).value);
      };
      return { props, slots, onInput };
    },
    template: `
      <label class="v-text-field">
        <span v-if="$slots.label"><slot name="label" /></span>
        <input
          data-testid="repository-search-input"
          :value="modelValue"
          @input="onInput" />
      </label>
    `,
  }),
};

describe("PublicRepositoryList.vue", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("fetches packages for advanced filter queries even when shorter than two characters", async () => {
    vi.useFakeTimers();
    (http.get as vi.Mock).mockResolvedValue({ data: [] });

    const wrapper = mount(PublicRepositoryList, {
      props: { repositories },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await wrapper.get('input[data-testid="repository-search-input"]').setValue("type:n");
    await vi.runAllTimersAsync();
    await flushPromises();

    expect(http.get).toHaveBeenCalledWith(
      "/api/search/packages",
      expect.objectContaining({
        params: expect.objectContaining({
          q: "type:n",
        }),
      }),
    );

    vi.useRealTimers();
  });

  it("opens the search help modal when the help button is clicked", async () => {
    const wrapper = mount(PublicRepositoryList, {
      props: { repositories },
      global: {
        stubs: vuetifyStubs,
      },
    });

    expect(wrapper.find('[data-testid="search-help-modal"]').exists()).toBe(false);
    await wrapper.get('[data-testid="search-help-button"]').trigger("click");
    expect(wrapper.find('[data-testid="search-help-modal"]').exists()).toBe(true);
  });

  it("applies example queries from the help modal", async () => {
    const wrapper = mount(PublicRepositoryList, {
      props: { repositories },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await wrapper.get('[data-testid="search-help-button"]').trigger("click");
    await wrapper.get('[data-testid="search-example-basic"]').trigger("click");

    const input = wrapper.get('input[data-testid="repository-search-input"]').element as HTMLInputElement;
    expect(input.value).toBe("gin");
    expect(wrapper.find('[data-testid="search-help-modal"]').exists()).toBe(false);
  });
});
