/**
 * Omega IDE Context Extension
 *
 * Works with VS Code and Cursor (which shares the VS Code extension API).
 * On meaningful editor events it sends structured context to the local Omega
 * API server (default: http://localhost:17421/api/ide-context).
 *
 * Privacy: all data stays on-device. The extension only talks to localhost.
 * The `omega.sendVisibleCode` setting (default off) controls whether the
 * visible code text is included.
 */

import * as vscode from "vscode";
import * as http from "http";
import * as path from "path";
import * as cp from "child_process";

// ── Helpers ───────────────────────────────────────────────────────────────────

function cfg<T>(key: string): T {
  return vscode.workspace.getConfiguration("omega").get<T>(key) as T;
}

function apiPort(): number {
  return cfg<number>("apiPort") ?? 17421;
}

function isEnabled(): boolean {
  return cfg<boolean>("enabled") ?? true;
}

function sendVisibleCode(): boolean {
  return cfg<boolean>("sendVisibleCode") ?? false;
}

/** Fire-and-forget POST to the local Omega API. Never throws. */
function postContext(payload: object): void {
  const body = JSON.stringify(payload);
  const options: http.RequestOptions = {
    hostname: "127.0.0.1",
    port: apiPort(),
    path: "/api/ide-context",
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Content-Length": Buffer.byteLength(body),
    },
  };
  const req = http.request(options);
  req.on("error", () => {
    // Omega not running — silently ignore.
  });
  req.write(body);
  req.end();
}

/** Run `git rev-parse --abbrev-ref HEAD` in `dir`. Returns null on failure. */
function gitBranch(dir: string): string | null {
  try {
    const result = cp.execSync("git rev-parse --abbrev-ref HEAD", {
      cwd: dir,
      encoding: "utf8",
      timeout: 2000,
      stdio: ["ignore", "pipe", "ignore"],
    });
    const branch = result.trim();
    return branch === "" || branch === "HEAD" ? null : branch;
  } catch {
    return null;
  }
}

/** Collect diagnostics for the active document (errors and warnings). */
function activeDiagnostics(uri: vscode.Uri): string[] {
  return vscode.languages
    .getDiagnostics(uri)
    .filter(
      (d) =>
        d.severity === vscode.DiagnosticSeverity.Error ||
        d.severity === vscode.DiagnosticSeverity.Warning
    )
    .slice(0, 10) // cap to avoid oversized payloads
    .map((d) => {
      const prefix =
        d.severity === vscode.DiagnosticSeverity.Error ? "Error" : "Warning";
      return `${prefix} [L${d.range.start.line + 1}]: ${d.message}`;
    });
}

/** Build and send the context payload for the active editor. */
function sendActiveContext(reason: string): void {
  if (!isEnabled()) return;

  const editor = vscode.window.activeTextEditor;
  if (!editor) return;

  const doc = editor.document;
  const workspaceFolder = vscode.workspace.getWorkspaceFolder(doc.uri);
  const workspacePath = workspaceFolder?.uri.fsPath ?? null;
  const workspaceName = workspaceFolder?.name ?? null;
  const activeFile = path.basename(doc.fileName);
  const languageId = doc.languageId;

  let visibleCode: string | null = null;
  if (sendVisibleCode()) {
    const visible = editor.visibleRanges[0];
    if (visible) {
      visibleCode = doc.getText(visible).slice(0, 2000); // 2 KB cap
    }
  }

  const branch = workspacePath ? gitBranch(workspacePath) : null;
  const diagnostics = activeDiagnostics(doc.uri);

  postContext({
    app_name: "Code", // VS Code process name on macOS
    ide_label: "VS Code",
    workspace: workspaceName,
    workspace_path: workspacePath,
    active_file: activeFile,
    language: languageId,
    git_branch: branch,
    visible_code: visibleCode,
    diagnostics: diagnostics.length > 0 ? diagnostics : null,
    _reason: reason,
  });
}

// ── Debounce ──────────────────────────────────────────────────────────────────

let debounceTimer: NodeJS.Timeout | undefined;

function scheduleContextPush(reason: string, delayMs = 1500): void {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = setTimeout(() => sendActiveContext(reason), delayMs);
}

// ── Activation ────────────────────────────────────────────────────────────────

export function activate(context: vscode.ExtensionContext): void {
  // Send on file save — most reliable signal that real work happened.
  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument(() =>
      scheduleContextPush("save", 0)
    )
  );

  // Send when the user switches to a different file.
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() =>
      scheduleContextPush("file-switch", 500)
    )
  );

  // Send when the active file's diagnostics change (new errors / fixes).
  context.subscriptions.push(
    vscode.languages.onDidChangeDiagnostics((e) => {
      const active = vscode.window.activeTextEditor?.document.uri;
      if (active && e.uris.some((u) => u.toString() === active.toString())) {
        scheduleContextPush("diagnostics-change", 2000);
      }
    })
  );

  // Initial push when the extension activates.
  scheduleContextPush("activate", 3000);
}

export function deactivate(): void {
  if (debounceTimer) clearTimeout(debounceTimer);
}
