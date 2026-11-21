// Runs tools/catalog/hxs_catalog.py on the user's firmware inside Pyodide. The firmware
// never leaves the browser: it is written to Pyodide's in-memory file system only.
var PYODIDE = "https://cdn.jsdelivr.net/pyodide/v0.26.4/full/";
importScripts(PYODIDE + "pyodide.js");

async function boot() {
  postMessage({ status: "loading Python (first time takes a while)..." });
  var py = await loadPyodide({ indexURL: PYODIDE });
  await py.loadPackage("numpy");
  var manifest = await (await fetch("files.json")).json();
  postMessage({ status: "loading the extractor and HXtra's tools (" + manifest.files.length + " files)..." });
  for (var f of manifest.files) {
    var r = await fetch(f);
    if (!r.ok) throw new Error("could not load " + f + ": " + r.status);
    var path = "/" + f;
    py.FS.mkdirTree(path.slice(0, path.lastIndexOf("/")));
    py.FS.writeFile(path, new Uint8Array(await r.arrayBuffer()));
  }
  postMessage({ status: "ready", hxtra_commit: manifest.hxtra_commit });
  return py;
}

var ready = boot();
ready.catch(function (e) { postMessage({ error: "could not start: " + e }); });

onmessage = async function (e) {
  try {
    var py = await ready;
    postMessage({ status: "reading the firmware..." });
    py.FS.mkdirTree("/work");
    py.FS.writeFile("/work/firmware.hxf", new Uint8Array(e.data.firmware));
    var args = ["/py/hxtra/tools", "/work/firmware.hxf", "/work/hxs-catalog.json"];
    if (e.data.stock) {
      py.FS.writeFile("/work/stock.hxf", new Uint8Array(e.data.stock));
      args.push("--stock", "/work/stock.hxf");
    }
    py.globals.set("ARGS", py.toPy(args));
    var summary = py.runPython([
      "import sys, io, contextlib",
      "sys.path.insert(0, '/py')",
      "import hxs_catalog",
      "_out = io.StringIO()",
      "with contextlib.redirect_stdout(_out):",
      "    hxs_catalog.main(list(ARGS))",
      "_out.getvalue()",
    ].join("\n"));
    var json = py.FS.readFile("/work/hxs-catalog.json", { encoding: "utf8" });
    py.FS.unlink("/work/firmware.hxf");
    if (e.data.stock) py.FS.unlink("/work/stock.hxf");
    postMessage({ done: true, summary: summary, json: json });
  } catch (err) {
    postMessage({ error: String(err) });
  }
};
