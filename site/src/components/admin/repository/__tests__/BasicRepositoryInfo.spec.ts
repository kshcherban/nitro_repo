import { describe, expect, it, vi, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";

vi.mock("@/http", () => ({
  default: {
    delete: vi.fn(),
  },
}));

vi.mock("@/router", () => ({
  default: {
    push: vi.fn(),
  },
}));

vi.mock("@kyvg/vue3-notification", () => ({
  notify: vi.fn(),
}));

import BasicRepositoryInfo from "../BasicRepositoryInfo.vue";

const vuetifyStubs = {
  "v-card": {
    template: `<div data-stub="v-card"><slot /></div>`,
  },
  "v-card-text": {
    template: `<div data-stub="v-card-text"><slot /></div>`,
  },
  "v-card-title": {
    template: `<div data-stub="v-card-title"><slot /></div>`,
  },
  "v-divider": {
    template: `<div data-stub="v-divider"><slot /></div>`,
  },
  "v-row": {
    template: `<div data-stub="v-row"><slot /></div>`,
  },
  "v-col": {
    template: `<div data-stub="v-col"><slot /></div>`,
  },
  "v-chip": {
    template: `<span data-stub="v-chip"><slot /></span>`,
  },
  "v-btn": {
    template: `<button data-stub="v-btn"><slot /></button>`,
  },
  "v-icon": {
    template: `<i data-stub="v-icon"><slot /></i>`,
  },
};

const repository = {
  id: "repository-123",
  name: "helm-charts",
  repository_type: "helm",
  storage_name: "s3-store",
  storage_id: "s3-store",
  storage_usage_bytes: 1024,
  storage_usage_updated_at: "2025-11-10T15:00:00Z",
  active: true,
  auth_enabled: true,
};

describe("BasicRepositoryInfo", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders repository metadata in a themed card layout", () => {
    const wrapper = mount(BasicRepositoryInfo, {
      props: { repository },
      global: {
        stubs: vuetifyStubs,
      },
    });

    expect(wrapper.find('[data-testid="repository-info-card"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="repository-status-chip"]').text()).toContain("Active");
    expect(wrapper.findAll('[data-testid="repository-meta-item"]').length).toBeGreaterThan(0);
  });

  it("provides themed action buttons for lifecycle operations", () => {
    const wrapper = mount(BasicRepositoryInfo, {
      props: { repository },
      global: {
        stubs: vuetifyStubs,
      },
    });

    expect(wrapper.find('[data-testid="repository-toggle"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="repository-delete"]').exists()).toBe(true);
  });
});
