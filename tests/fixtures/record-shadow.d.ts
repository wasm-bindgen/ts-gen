// A local `Record` must not be promoted to a record binding. The rendered
// `Object<Number>` is known-broken: references to `Record<…>` still hit the
// builtin js_sys mapping instead of the local shadow, which is pre-existing
// generic-alias behaviour unrelated to record bindings.
type Record<K, V> = V;

export type ShadowedRecord = Record<string, number>;
