// One generator instance. The page starts one worker per core; each loads
// the WebAssembly module, whose game data is built in (standalone build),
// and generates worlds on request.
importScripts("ds-worldgen-web.js");

let m = null;

function call(name, args = [], types = []) {
  const ptr = m.ccall(name, "number", types, args);
  if (!ptr) return null;
  const text = m.UTF8ToString(ptr);
  m.ccall("dsw_free", null, ["number"], [ptr]);
  return text;
}

onmessage = async (e) => {
  const msg = e.data;
  try {
    if (msg.type === "init") {
      // The page compiles the module once and shares it with every worker.
      const options = { print: () => {}, printErr: () => {} };
      if (msg.module) {
        options.instantiateWasm = (imports, receive) => {
          WebAssembly.instantiate(msg.module, imports).then((instance) => receive(instance, msg.module));
          return {};
        };
      }
      if (!m) m = await createDsWorldgen(options);
      postMessage({ id: msg.id, info: JSON.parse(call("dsw_embedded")) });
    } else if (msg.type === "assets") {
      postMessage({ id: msg.id, assets: JSON.parse(call("dsw_assets", [msg.game], ["string"])) });
    } else if (msg.type === "generate") {
      postMessage({ id: msg.id, result: JSON.parse(call("dsw_generate", [JSON.stringify(msg.request)], ["string"])) });
    }
  } catch (err) {
    postMessage({ id: msg.id, result: { error: String((err && err.message) || err) } });
  }
};
