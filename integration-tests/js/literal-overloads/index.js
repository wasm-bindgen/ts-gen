const seen = (options) =>
  Object.keys(options)
    .sort()
    .map((k) => `${k}=${options[k]}`)
    .join(",");

export const store = () => ({
  async read(key, options) {
    if (options === undefined) return key;
    const text = `${key}:${seen(options)}`;
    return options.type === "arrayBuffer" ? new TextEncoder().encode(text).buffer : text;
  },
  tail: seen,
  open: seen,
});
