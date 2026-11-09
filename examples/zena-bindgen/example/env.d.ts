// The worker's bindings from config.capnp, merged into the fixture's `Env`.
interface Env {
  readonly GREETING: string;
}
interface Env {
  readonly COUNTER: CounterNamespace;
}

// workerd's namespace binding isn't an instance of a global
// `DurableObjectNamespace`, which the bridge would check for the class
// workers-types declares; an interface isn't checked.
interface CounterNamespace {
  getByName(name: string): DurableObjectStub;
}
