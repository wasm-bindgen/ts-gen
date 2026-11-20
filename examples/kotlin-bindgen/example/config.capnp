using Workerd = import "/workerd/workerd.capnp";
const config :Workerd.Config = (
  services = [
    (name = "main", worker = (
      modules = [
        (name = "loader.js", esModule = embed "loader.js"),
        (name = "bridge.js", esModule = embed "bridge.js"),
        (name = "bridge.wasm", wasm = embed "bridge.wasm"),
        (name = "worker.import-object.mjs", esModule = embed "worker.import-object.mjs"),
        (name = "worker.js-builtins.mjs", esModule = embed "worker.js-builtins.mjs"),
        (name = "worker.wasm", wasm = embed "worker.wasm"),
      ],
      bindings = [
        (name = "GREETING", text = "Hello from workerd"),
        (name = "BACKEND", service = "backend"),
        (name = "COUNTER", durableObjectNamespace = "Counter"),
        (name = "GREETER", service = (name = "main", entrypoint = "Greeter")),
        (name = "SELF", service = "main"),
      ],
      durableObjectNamespaces = [(className = "Counter", uniqueKey = "kotlin-worker-counter", enableSql = true)],
      durableObjectStorage = (localDisk = "storage"),
      globalOutbound = "backend",
      compatibilityDate = "2026-03-17",
      compatibilityFlags = ["service_binding_extra_handlers"],
    )),
    (name = "storage", disk = (path = "storage", writable = true)),
    (name = "backend", worker = (
      modules = [(name = "backend.js", esModule = embed "backend.js")],
      compatibilityDate = "2026-03-17",
      compatibilityFlags = ["enable_request_signal"],
    )),
  ],
  sockets = [(name = "http", address = "127.0.0.1:8789", http = (), service = "main")],
);
