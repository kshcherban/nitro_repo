import { config, flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/http", () => ({
  default: {
    get: vi.fn(),
  },
}));

import RepositoryPackagesPublic from "@/components/nr/repository/RepositoryPackagesPublic.vue";
import http from "@/http";

function createPackages(items: any[] = [], total = items.length, headers: Record<string, string> = {}) {
  return {
    data: {
      items,
      total_packages: total,
    },
    headers,
  };
}

function createLocalStorageStub() {
  let store: Record<string, string> = {};
  return {
    getItem(key: string) {
      return Object.prototype.hasOwnProperty.call(store, key) ? store[key] : null;
    },
    setItem(key: string, value: string) {
      store[key] = String(value);
    },
    removeItem(key: string) {
      delete store[key];
    },
    clear() {
      store = {};
    },
    key(index: number) {
      return Object.keys(store)[index] ?? null;
    },
    get length() {
      return Object.keys(store).length;
    },
  };
}

const vBtnStub = defineComponent({
    name: "VBtnStub",
    emits: ["click"],
    setup(_, { slots, emit }) {
      return () =>
        h(
          "button",
          {
            "data-stub": "v-btn",
            type: "button",
            onClick: (event: Event) => emit("click", event),
          },
          slots.default?.(),
        );
    },
  });

const vSelectStub = defineComponent({
    name: "VSelectStub",
    props: {
      modelValue: {
        type: [String, Number, Array, Object],
        default: undefined,
      },
      items: {
        type: Array,
        default: () => [],
      },
    },
    emits: ["update:modelValue"],
    setup(props, { emit }) {
      return () =>
        h(
          "select",
          {
            "data-stub": "v-select",
            value: props.modelValue as any,
            onChange: (event: Event) => {
              const target = event.target as HTMLSelectElement;
              emit("update:modelValue", target.value);
            },
          },
          (props.items as any[]).map((item) =>
            h("option", { value: item }, item),
          ),
        );
    },
  });

const vuetifyStubs = {
  "v-btn": vBtnStub,
  VBtn: vBtnStub,
  "v-select": vSelectStub,
  VSelect: vSelectStub,
};

config.global.stubs = {
  ...config.global.stubs,
  ...vuetifyStubs,
};

describe("RepositoryPackagesPublic.vue", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.stubGlobal("localStorage", createLocalStorageStub());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("requests packages with per_page 100 by default", async () => {
    (http.get as vi.Mock).mockResolvedValue(createPackages());

    mount(RepositoryPackagesPublic, {
      props: {
        repositoryId: "repo-123",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    expect(http.get).toHaveBeenCalledWith(
      "/api/repository/repo-123/packages",
      expect.objectContaining({
        params: expect.objectContaining({
          per_page: 100,
          page: 1,
        }),
      }),
    );
  });

  it("sorts the current page by column when headers are clicked", async () => {
    (http.get as vi.Mock).mockResolvedValue(
      createPackages([
        {
          package: "pkg-beta",
          name: "Beta",
          size: 2048,
          cache_path: "cache/pkg-beta",
          modified: "2025-11-05T09:30:00Z",
        },
        {
          package: "pkg-alpha",
          name: "Alpha",
          size: 1024,
          cache_path: "cache/pkg-alpha",
          modified: "2025-11-06T11:45:00Z",
        },
        {
          package: "pkg-gamma",
          name: "Gamma",
          size: 1536,
          cache_path: "cache/pkg-gamma",
          modified: "2025-11-04T18:15:00Z",
        },
      ]),
    );

    const wrapper = mount(RepositoryPackagesPublic, {
      props: {
        repositoryId: "repo-123",
        repositoryType: "python",
        repositoryKind: "proxy",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    const rowOrder = () =>
      wrapper
        .findAll('[data-testid="package-row"]')
        .map((row) => row.find('[data-testid="package-cell"]').text().trim());

    expect(rowOrder()).toEqual(["pkg-beta", "pkg-alpha", "pkg-gamma"]);

    await wrapper.get('[data-testid="sort-size"]').trigger("click");
    await flushPromises();
    expect(rowOrder()).toEqual(["pkg-alpha", "pkg-gamma", "pkg-beta"]);

    await wrapper.get('[data-testid="sort-size"]').trigger("click");
    await flushPromises();
    expect(rowOrder()).toEqual(["pkg-beta", "pkg-gamma", "pkg-alpha"]);
  });

  it("filters packages with the inline search input", async () => {
    (http.get as vi.Mock).mockResolvedValue(
      createPackages([
        {
          package: "pkg-one",
          name: "One",
          size: 1024,
          cache_path: "cache/pkg-one",
          modified: "2025-11-05T09:30:00Z",
        },
        {
          package: "pkg-two",
          name: "Two",
          size: 2048,
          cache_path: "cache/pkg-two",
          modified: "2025-11-05T11:30:00Z",
        },
      ]),
    );

    const wrapper = mount(RepositoryPackagesPublic, {
      props: {
        repositoryId: "repo-xyz",
        repositoryType: "python",
        repositoryKind: "proxy",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();
    expect(wrapper.findAll('[data-testid="package-row"]')).toHaveLength(2);

    await wrapper.get('[data-testid="packages-search-input"]').setValue("two");
    await flushPromises();

    const rows = wrapper.findAll('[data-testid="package-row"]');
    expect(rows).toHaveLength(1);
    expect(rows[0].find('[data-testid="package-cell"]').text()).toBe("pkg-two");
  });

  it("shows indexing warning when backend signals indexing", async () => {
    (http.get as vi.Mock).mockResolvedValue(
      createPackages([], 0, { "x-nitro-warning": "Repository indexing in progress" }),
    );

    const wrapper = mount(RepositoryPackagesPublic, {
      props: {
        repositoryId: "repo-123",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    const warning = wrapper.find('[data-testid="public-packages-indexing-warning"]');
    expect(warning.exists()).toBe(true);
    expect(warning.text()).toContain("Repository indexing in progress");
  });

  it("persists column visibility preferences per repository", async () => {
    (http.get as vi.Mock).mockResolvedValue(
      createPackages([
        {
          package: "pkg-alpha",
          name: "Alpha",
          size: 1024,
          cache_path: "cache/pkg-alpha",
          modified: "2025-11-06T11:45:00Z",
        },
      ]),
    );

    const wrapper = mount(RepositoryPackagesPublic, {
      props: {
        repositoryId: "repo-ABC",
        repositoryType: "python",
        repositoryKind: "proxy",
      },
      global: {
        stubs: vuetifyStubs,
      },
    });

    await flushPromises();

    await wrapper.get('[data-testid="packages-config-toggle"]').trigger("click");
    const pathToggle = wrapper.get('[data-testid="toggle-path"]');
    expect((pathToggle.element as HTMLInputElement).checked).toBe(true);

    await pathToggle.setValue(false);
    await flushPromises();

    expect(wrapper.find('th[data-column="path"]').exists()).toBe(false);

    wrapper.unmount();

    (http.get as vi.Mock).mockResolvedValue(
      createPackages([
        {
          package: "pkg-beta",
          name: "Beta",
          size: 512,
          cache_path: "cache/pkg-beta",
          modified: "2025-11-01T08:00:00Z",
        },
      ]),
    );

    const wrapperAgain = mount(RepositoryPackagesPublic, {
      props: {
        repositoryId: "repo-ABC",
        repositoryType: "python",
        repositoryKind: "proxy",
      },
    });

    await flushPromises();
    expect(wrapperAgain.find('th[data-column="path"]').exists()).toBe(false);
  });
});
