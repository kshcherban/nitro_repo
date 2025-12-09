const storageFactory = () => {
  let store: Record<string, string> = {};
  return {
    get length() {
      return Object.keys(store).length;
    },
    key(index: number) {
      const keys = Object.keys(store);
      return keys[index] ?? null;
    },
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
  };
};

Object.defineProperty(globalThis, "localStorage", {
  value: storageFactory(),
  configurable: true,
  writable: true,
});

Object.defineProperty(globalThis, "sessionStorage", {
  value: storageFactory(),
  configurable: true,
  writable: true,
});
