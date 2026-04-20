# Omega IDE Context — VS Code / Cursor Extension

Sends structured code context to the local Omega capture engine so your coding
sessions appear in your daily summaries — alongside anything you wrote in Word
or Slack about the same work.

## How it works

- On **file save**, **file switch**, and **diagnostic change**, the extension
  sends a small JSON payload to `http://localhost:17421/api/ide-context`.
- The payload includes: active file name, language, git branch, workspace name,
  and (optionally) the visible code in the editor viewport.
- All data stays **on-device**. The extension only talks to localhost.

## Installation

```bash
cd extensions/omega-vscode
npm install
npm run compile
```

Then install the compiled extension in VS Code / Cursor via
**Extensions → Install from VSIX** (after running `npm run package`), or
symlink the folder into your VS Code extensions directory for development.

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `omega.apiPort` | `17421` | Port the local Omega API server listens on |
| `omega.enabled` | `true` | Enable / disable context capture |
| `omega.sendVisibleCode` | `false` | Include visible editor code in the payload. Disable for sensitive work. |

## JetBrains / other IDEs

The window-title parsing in the Omega capture engine already detects IntelliJ,
PyCharm, WebStorm, CLion, GoLand, Rider, and more — no plugin needed for basic
context. A richer JetBrains plugin following the same pattern as this extension
can be added separately.
