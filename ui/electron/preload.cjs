const { contextBridge } = require("electron");

const port = process.env.OMEGA_API_PORT || "17421";
contextBridge.exposeInMainWorld("omega", {
  apiBase: `http://127.0.0.1:${port}`,
});
