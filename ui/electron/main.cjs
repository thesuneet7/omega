const {
  app,
  BrowserWindow,
  Tray,
  Menu,
  nativeImage,
  globalShortcut,
} = require("electron");
const path = require("path");
const fs = require("fs");
const { spawn } = require("child_process");
const http = require("http");

const API_PORT = process.env.OMEGA_API_PORT || "17421";
const API_HOST = "127.0.0.1";
const GLOBAL_PAUSE_ACCEL = "CommandOrControl+Shift+9";

let apiChild = null;
let tray = null;
let capturePaused = false;

function repoRoot() {
  return path.join(__dirname, "..", "..");
}

function omegaApiBinary() {
  const root = repoRoot();
  const name = process.platform === "win32" ? "omega-api.exe" : "omega-api";
  return path.join(root, "target", "debug", name);
}

function apiUrl(p) {
  return `http://${API_HOST}:${API_PORT}${p}`;
}

function waitForHealth(timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const tryOnce = () => {
      const req = http.get(apiUrl("/health"), (res) => {
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

function httpGetJson(urlPath) {
  return new Promise((resolve, reject) => {
    const req = http.get(apiUrl(urlPath), (res) => {
      let body = "";
      res.on("data", (c) => {
        body += c;
      });
      res.on("end", () => {
        if (res.statusCode !== 200) {
          reject(new Error(`${urlPath} HTTP ${res.statusCode}: ${body}`));
          return;
        }
        try {
          resolve(JSON.parse(body));
        } catch (e) {
          reject(e);
        }
      });
    });
    req.on("error", reject);
    req.setTimeout(8000, () => {
      req.destroy();
      reject(new Error(`${urlPath} timeout`));
    });
  });
}

function httpPutJson(urlPath, payload) {
  const data = JSON.stringify(payload);
  return new Promise((resolve, reject) => {
    const req = http.request(
      apiUrl(urlPath),
      {
        method: "PUT",
        headers: {
          "Content-Type": "application/json",
          "Content-Length": Buffer.byteLength(data),
        },
      },
      (res) => {
        let body = "";
        res.on("data", (c) => {
          body += c;
        });
        res.on("end", () => {
          if (res.statusCode !== 200) {
            reject(new Error(`${urlPath} HTTP ${res.statusCode}: ${body}`));
            return;
          }
          try {
            resolve(JSON.parse(body));
          } catch (e) {
            reject(e);
          }
        });
      }
    );
    req.on("error", reject);
    req.write(data);
    req.end();
  });
}

async function fetchLiveStatus() {
  return httpGetJson("/api/capture/live-status");
}

async function putCapturePaused(paused) {
  return httpPutJson("/api/capture/pause", { paused });
}

function buildTrayMenu() {
  return Menu.buildFromTemplate([
    {
      label: capturePaused ? "Resume capture" : "Pause capture",
      click: async () => {
        try {
          const next = !capturePaused;
          const s = await putCapturePaused(next);
          capturePaused = !!s.capturePaused;
          syncTrayChrome();
          tray?.setContextMenu(buildTrayMenu());
        } catch (e) {
          console.error("Omega tray: pause toggle failed", e);
        }
      },
    },
    { type: "separator" },
    {
      label: "Show Omega",
      click: () => {
        const wins = BrowserWindow.getAllWindows();
        if (wins[0]) {
          wins[0].show();
          wins[0].focus();
        } else {
          createWindow();
        }
      },
    },
    { type: "separator" },
    {
      label: "Quit",
      click: () => app.quit(),
    },
  ]);
}

function syncTrayChrome() {
  if (!tray) return;
  tray.setToolTip(
    capturePaused
      ? "Omega — capture paused (⌘⇧9)"
      : "Omega — pause/resume capture (⌘⇧9)"
  );
}

async function refreshPauseFromApi() {
  try {
    const s = await fetchLiveStatus();
    capturePaused = !!s.capturePaused;
    syncTrayChrome();
    tray?.setContextMenu(buildTrayMenu());
  } catch {
    /* e.g. first launch before any live-status file */
  }
}

function createTray() {
  const iconPath = path.join(__dirname, "trayTemplate.png");
  let image;
  if (fs.existsSync(iconPath)) {
    image = nativeImage.createFromPath(iconPath);
    if (process.platform === "darwin" && !image.isEmpty()) {
      image.setTemplateImage(true);
    }
  } else {
    image = nativeImage.createEmpty();
  }
  tray = new Tray(image);
  tray.setIgnoreDoubleClickEvents(true);
  capturePaused = false;
  syncTrayChrome();
  tray.setContextMenu(buildTrayMenu());
  void refreshPauseFromApi();
  setInterval(() => {
    void refreshPauseFromApi();
  }, 4000);
}

function registerGlobalPauseShortcut() {
  const ok = globalShortcut.register(GLOBAL_PAUSE_ACCEL, async () => {
    try {
      const s = await fetchLiveStatus();
      capturePaused = !!s.capturePaused;
      const next = !capturePaused;
      const out = await putCapturePaused(next);
      capturePaused = !!out.capturePaused;
      syncTrayChrome();
      tray?.setContextMenu(buildTrayMenu());
    } catch (e) {
      console.error("Omega: global pause shortcut failed", e);
    }
  });
  if (!ok) {
    console.warn(`Omega: could not register global shortcut ${GLOBAL_PAUSE_ACCEL}`);
  }
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
  createTray();
  registerGlobalPauseShortcut();
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

app.on("will-quit", () => {
  globalShortcut.unregisterAll();
});

app.on("before-quit", () => {
  if (apiChild && !apiChild.killed) {
    apiChild.kill();
  }
});
