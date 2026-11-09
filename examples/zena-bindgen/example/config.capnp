using Workerd = import "/workerd/workerd.capnp";
const config :Workerd.Config = (
  services = [(name = "main", worker = (
    modules = [
      (name = "loader.js", esModule = embed "loader.js"),
      (name = "bridge.js", esModule = embed "bridge.js"),
      (name = "bridge.wasm", wasm = embed "bridge.wasm"),
      (name = "worker.wasm", wasm = embed "worker.wasm"),
    ],
    bindings = [
      (name = "GREETING", text = "Hello from workerd"),
      (name = "COUNTER", durableObjectNamespace = "Counter"),
    ],
    durableObjectNamespaces = [(className = "Counter", uniqueKey = "zena-worker-counter")],
    durableObjectStorage = (inMemory = void),
    compatibilityDate = "2026-03-17",
    compatibilityFlags = ["wasm_esm_integration_builtins"],
  ))],
  sockets = [(name = "http", address = "127.0.0.1:8788", http = (), service = "main")],
);
