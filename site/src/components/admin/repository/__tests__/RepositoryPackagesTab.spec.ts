import { flushPromises, mount } from "@vue/test-utils";
import { describe, expect, it, vi } from "vitest";
import { defineComponent, nextTick } from "vue";
import RepositoryPackagesTab from "@/components/admin/repository/RepositoryPackagesTab.vue";
import http from "@/http";

vi.mock("@/http", () => ({
  default: {
    get: vi.fn().mockResolvedValue({
      data: {
        total_packages: 1,
        items: [
          {
            name: "express",
            size: 1024,
            cache_path: "/pkg/express-1.0.0.tgz",
            modified: "2024-01-01T00:00:00Z",
            package: "express",
          },
        ],
      },
    }),
    delete: vi.fn(),
  },
}));

const mockAlerts = {
  success: vi.fn(),
  error: vi.fn(),
};

vi.mock("@/stores/alerts", () => ({
  useAlertsStore: () => mockAlerts,
}));

vi.mock("@/composables/useResizableColumns", () => ({
  useResizableColumns: vi.fn(),
}));

const VCardStub = defineComponent({
  template: "<div class='v-card'><slot /></div>",
});

const VCardTitleStub = defineComponent({
  template: "<div class='v-card-title'><slot /></div>",
});

const VCardSubtitleStub = defineComponent({
  template: "<div class='v-card-subtitle'><slot /></div>",
});

const VCardTextStub = defineComponent({
  template: "<div class='v-card-text'><slot /></div>",
});

const VCardActionsStub = defineComponent({
  template: "<div class='v-card-actions'><slot /></div>",
});

const VSpacerStub = defineComponent({
  template: "<span class='v-spacer' />",
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

const VBtnStub = defineComponent({
  emits: ["click"],
  template: "<button class='v-btn' @click=\"$emit('click')\"><slot /></button>",
});

const VDataTableStub = defineComponent({
  props: {
    headers: Array,
    items: Array,
  },
  template: "<table class='v-data-table'><slot /></table>",
});

const VProgressCircularStub = defineComponent({
  template: "<div class='v-progress-circular'><slot /></div>",
});

const VIconStub = defineComponent({
  template: "<i class='v-icon'><slot /></i>",
});

const VPaginationStub = defineComponent({
  props: {
    modelValue: Number,
    length: Number,
  },
  emits: ["update:modelValue"],
  template: "<div class='v-pagination'><slot /></div>",
});

const VSelectStub = defineComponent({
  props: {
    modelValue: [String, Number],
    items: Array,
  },
  emits: ["update:modelValue"],
  template: "<select class='v-select'><slot /></select>",
});

const vuetifyStubs = {
  "v-card": VCardStub,
  "v-card-title": VCardTitleStub,
  "v-card-subtitle": VCardSubtitleStub,
  "v-card-text": VCardTextStub,
  "v-card-actions": VCardActionsStub,
  "v-spacer": VSpacerStub,
  "v-text-field": VTextFieldStub,
  "v-btn": VBtnStub,
  "v-data-table": VDataTableStub,
  "v-progress-circular": VProgressCircularStub,
  "v-icon": VIconStub,
  "v-pagination": VPaginationStub,
  "v-select": VSelectStub,
};

describe("RepositoryPackagesTab.vue", () => {
  it("marks search field clearable and clears search term", async () => {
    const wrapper = mount(RepositoryPackagesTab, {
      props: {
        repositoryId: "1",
        repositoryType: "npm",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    const field = wrapper.getComponent(VTextFieldStub);
    expect(field.props("clearable")).toBe(true);

    field.vm.$emit("update:modelValue", "express");
    await nextTick();
    expect(wrapper.vm.searchTerm).toBe("express");

    field.vm.$emit("click:clear");
    await nextTick();
    expect(wrapper.vm.searchTerm).toBe("");
  });

  it("reloads packages when items per page changes", async () => {
    const wrapper = mount(RepositoryPackagesTab, {
      props: {
        repositoryId: "1",
        repositoryType: "npm",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    const httpGet = http.get as vi.Mock;
    httpGet.mockClear();

    (wrapper.vm as any).currentPage = 2;
    (wrapper.vm as any).handleItemsPerPageChange(100);

    await flushPromises();

    expect(httpGet).toHaveBeenCalledTimes(1);
    expect(httpGet).toHaveBeenCalledWith("/api/repository/1/packages", {
      params: { page: 1, per_page: 100 },
    });
  });
});
