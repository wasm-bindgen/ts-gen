using Workerd = import "/workerd/workerd.capnp";
const config :Workerd.Config = (
  services = [(name = "main", worker = (
    modules = [
      (name = "loader.js", esModule = embed "loader.js"),
      (name = "builtins.js", esModule = embed "builtins.js"),
      (name = "main.wasm", wasm = embed "main.wasm"),
    ],
    bindings = [(name = "GREETING", text = "Hello from workerd")],
    compatibilityDate = "2026-03-17",
    compatibilityFlags = ["wasm_esm_integration_builtins"],
  ))],
  sockets = [(name = "http", address = "127.0.0.1:8788", http = (), service = "main")],
);
