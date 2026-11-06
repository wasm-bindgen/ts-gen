using Workerd = import "/workerd/workerd.capnp";
const config :Workerd.Config = (
  services = [(name = "main", worker = (
    modules = [
      (name = "loader.js", esModule = embed "loader.js"),
      (name = "builtins.js", esModule = embed "builtins.js"),
      (name = "worker.import-object.mjs", esModule = embed "worker.import-object.mjs"),
      (name = "worker.js-builtins.mjs", esModule = embed "worker.js-builtins.mjs"),
      (name = "worker.wasm", wasm = embed "worker.wasm"),
    ],
    bindings = [(name = "GREETING", text = "Hello from workerd")],
    compatibilityDate = "2026-03-17",
  ))],
  sockets = [(name = "http", address = "127.0.0.1:8789", http = (), service = "main")],
);
