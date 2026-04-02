const { app, BrowserWindow } = require("electron");
const path = require("path");
const { spawn } = require("child_process");
const http = require("http");

const API_PORT = process.env.OMEGA_API_PORT || "17421";
const API_HOST = "127.0.0.1";

let apiChild = null;

function repoRoot() {
  return path.join(__dirname, "..", "..");
}

function omegaApiBinary() {
  const root = repoRoot();
  const name = process.platform === "win32" ? "omega-api.exe" : "omega-api";
  return path.join(root, "target", "debug", name);
}

function waitForHealth(timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const tryOnce = () => {
      const req = http.get(`http://${API_HOST}:${API_PORT}/health`, (res) => {
        if (res.statusCode === 200) {
          resolve();
          return;
        }
        if (Date.now() - start > timeoutMs) {
          reject(new Error("omega-api health check failed (bad status)"));
          return;
        }
        setTimeout(tryOnce, 100);
      });
      req.on("error", () => {
        if (Date.now() - start > timeoutMs) {
          reject(new Error("omega-api did not become ready in time"));
          return;
        }
        setTimeout(tryOnce, 100);
      });
    };
    tryOnce();
  });
}

function startOmegaApi() {
  const bin = omegaApiBinary();
  apiChild = spawn(bin, [], {
    cwd: repoRoot(),
    env: { ...process.env, OMEGA_API_PORT: API_PORT },
    stdio: "inherit",
  });
  apiChild.on("exit", (code) => {
    if (code !== 0 && code !== null) {
      console.error(`omega-api exited with code ${code}`);
    }
  });
}

function createWindow() {
  const win = new BrowserWindow({
    width: 1280,
    height: 820,
    title: "Omega Session App",
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  const devUrl = process.env.VITE_DEV_SERVER_URL || "http://localhost:5174";
  if (!app.isPackaged) {
    win.loadURL(devUrl);
    win.webContents.openDevTools({ mode: "detach" });
  } else {
    win.loadFile(path.join(__dirname, "..", "dist", "index.html"));
  }
}

app.whenReady().then(async () => {
  startOmegaApi();
  try {
    await waitForHealth();
  } catch (e) {
    console.error(e);
    app.quit();
    return;
  }
  createWindow();
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    app.quit();
  }
});

app.on("before-quit", () => {
  if (apiChild && !apiChild.killed) {
    apiChild.kill();
  }
});
