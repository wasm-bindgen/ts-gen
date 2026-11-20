using Workerd = import "/workerd/workerd.capnp";

const config :Workerd.Config = (
  services = [
    (name = "main", worker = (
      modules = [
        (name = "loader.js", esModule = embed "loader.js"),
        (name = "bridge.js", esModule = embed "bridge.js"),
        (name = "bridge.wasm", wasm = embed "bridge.wasm"),
        (name = "worker.wasm", wasm = embed "worker.wasm"),
      ],
      bindings = [
        (name = "GREETING", text = "Hello from workerd"),
        (name = "HELPER", service = "helper"),
        (name = "COUNTER", durableObjectNamespace = "Counter"),
        (name = "ROOM", durableObjectNamespace = "Room"),
        (name = "GREETER", service = (name = "main", entrypoint = "Greeter")),
      ],
      durableObjectNamespaces = [
        (className = "Counter", uniqueKey = "counter", enableSql = true),
        (className = "Room", uniqueKey = "room"),
      ],
      durableObjectStorage = (localDisk = "storage"),
      globalOutbound = "helper",
      cacheApiOutbound = "helper",
      compatibilityDate = "2026-03-17",
      compatibilityFlags = ["wasm_esm_integration_builtins"],
    )),
    (name = "helper", worker = (
      modules = [(name = "helper.js", esModule = embed "helper.js")],
      bindings = [(name = "MAIN", service = "main")],
      compatibilityDate = "2026-03-17",
      compatibilityFlags = ["service_binding_extra_handlers"],
    )),
    (name = "storage", disk = (path = "storage", writable = true)),
  ],
  sockets = [(name = "http", address = "127.0.0.1:8788", http = (), service = "main")],
);
