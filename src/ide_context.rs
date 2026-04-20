//! IDE-aware context extraction.
//!
//! Three IDE-agnostic mechanisms:
//!   1. Window-title parsing — a rule table covers every named editor; new IDEs
//!      are added by appending a row, no logic changes required.
//!   2. Git branch detection — runs `git rev-parse` in the workspace directory;
//!      works identically regardless of which editor the user has open.
//!   3. Language inference from file extension — purely local, no network.
//!
//! The VS Code extension (Layer C) pushes richer context directly to the API
//! and creates a `VisualLogItem` with these fields pre-populated.

use crate::models::IdeContext;
use std::path::Path;
use std::process::Command;

// ── IDE recognition table ─────────────────────────────────────────────────────

struct IdeWindowRule {
    /// Substring matched case-insensitively against the macOS process name.
    app_name_substr: &'static str,
    parser: TitleParser,
}

#[derive(Clone, Copy)]
enum TitleParser {
    /// `file — workspace — App`  (VS Code, Cursor, Xcode, Zed, …)
    EmDashSeparated,
    /// `file [workspace] - App`  (JetBrains family)
    BracketWorkspace,
    /// `file - workspace - App`  (Sublime Text, Nova, …)
    HyphenSeparated,
    /// `file - App` or just `App` (RStudio, Vim, Emacs, …)
    SimpleFile,
}

static IDE_RULES: &[IdeWindowRule] = &[
    // VS Code ships as process "Code"; Cursor ships as "Cursor"
    IdeWindowRule { app_name_substr: "visual studio code", parser: TitleParser::EmDashSeparated },
    IdeWindowRule { app_name_substr: "code",               parser: TitleParser::EmDashSeparated },
    IdeWindowRule { app_name_substr: "cursor",             parser: TitleParser::EmDashSeparated },
    IdeWindowRule { app_name_substr: "xcode",              parser: TitleParser::EmDashSeparated },
    IdeWindowRule { app_name_substr: "zed",                parser: TitleParser::EmDashSeparated },
    // JetBrains family
    IdeWindowRule { app_name_substr: "intellij",           parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "pycharm",            parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "webstorm",           parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "clion",              parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "goland",             parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "rubymine",           parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "datagrip",           parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "rider",              parser: TitleParser::BracketWorkspace },
    IdeWindowRule { app_name_substr: "android studio",     parser: TitleParser::BracketWorkspace },
    // Hyphen-separated
    IdeWindowRule { app_name_substr: "sublime text",       parser: TitleParser::HyphenSeparated },
    IdeWindowRule { app_name_substr: "sublime",            parser: TitleParser::HyphenSeparated },
    IdeWindowRule { app_name_substr: "nova",               parser: TitleParser::HyphenSeparated },
    IdeWindowRule { app_name_substr: "visual studio",      parser: TitleParser::HyphenSeparated },
    // Simple / fallback
    IdeWindowRule { app_name_substr: "rstudio",            parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "vim",                parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "nvim",               parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "neovim",             parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "emacs",              parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "helix",              parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "textmate",           parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "eclipse",            parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "netbeans",           parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "matlab",             parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "spyder",             parser: TitleParser::SimpleFile },
    IdeWindowRule { app_name_substr: "jupyter",            parser: TitleParser::SimpleFile },
];

/// Returns `true` if the app name matches any known code editor.
#[allow(dead_code)]
pub fn is_ide_app(app_name: &str) -> bool {
    let lower = app_name.to_ascii_lowercase();
    IDE_RULES.iter().any(|r| lower.contains(r.app_name_substr))
}

/// Attempts to extract structured IDE context from the active app name and
/// window title. Returns `None` if the app is not a recognised editor.
pub fn parse_ide_context(app_name: &str, window_title: &str) -> Option<IdeContext> {
    let lower_app = app_name.to_ascii_lowercase();
    let rule = IDE_RULES
        .iter()
        .find(|r| lower_app.contains(r.app_name_substr))?;

    let (active_file, workspace) = match rule.parser {
        TitleParser::EmDashSeparated  => parse_emdash(window_title, rule.app_name_substr),
        TitleParser::BracketWorkspace => parse_bracket(window_title),
        TitleParser::HyphenSeparated  => parse_hyphen(window_title, rule.app_name_substr),
        TitleParser::SimpleFile       => parse_simple(window_title, rule.app_name_substr),
    };

    let language     = active_file.as_deref().and_then(language_from_filename).map(str::to_string);
    let git_branch   = workspace.as_deref().and_then(get_git_branch);
    let workspace_path = workspace.clone();

    Some(IdeContext { active_file, workspace, language, git_branch, workspace_path })
}

// ── Title parsers ─────────────────────────────────────────────────────────────

/// `payment.ts — omega — Visual Studio Code`
fn parse_emdash(title: &str, app_substr: &str) -> (Option<String>, Option<String>) {
    let title = strip_app_suffix(title, app_substr);
    let parts: Vec<&str> = title.split('\u{2014}').collect(); // '—'
    let file      = clean(parts.first().copied());
    let workspace = clean(parts.get(1).copied());
    (file, workspace)
}

/// `payment.java [omega] - IntelliJ IDEA`
fn parse_bracket(title: &str) -> (Option<String>, Option<String>) {
    if let (Some(bs), Some(be)) = (title.find('['), title.find(']')) {
        if bs < be {
            let file      = clean(Some(&title[..bs]));
            let workspace = clean(Some(&title[bs + 1..be]));
            return (file, workspace);
        }
    }
    parse_hyphen(title, "")
}


/// `payment.py - omega - Sublime Text`
fn parse_hyphen(title: &str, app_substr: &str) -> (Option<String>, Option<String>) {
    let title = strip_app_suffix(title, app_substr);
    let parts: Vec<&str> = title.split(" - ").collect();
    let file      = clean(parts.first().copied());
    let workspace = clean(parts.get(1).copied());
    (file, workspace)
}

/// `payment.R - RStudio`  →  just the file part
fn parse_simple(title: &str, app_substr: &str) -> (Option<String>, Option<String>) {
    let stripped = strip_app_suffix(title, app_substr);
    let file = if let Some((left, _)) = stripped.split_once(" - ") {
        clean(Some(left))
    } else {
        clean(Some(stripped))
    };
    (file, None)
}

fn clean(s: Option<&str>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// Strip the rightmost ` — AppName` or ` - AppName` segment that contains `app_substr`.
fn strip_app_suffix<'a>(title: &'a str, app_substr: &str) -> &'a str {
    if app_substr.is_empty() {
        return title;
    }
    for sep in [" \u{2014} ", " - "] {
        if let Some(idx) = title.rfind(sep) {
            if title[idx + sep.len()..].to_ascii_lowercase().contains(app_substr) {
                return &title[..idx];
            }
        }
    }
    title
}

// ── Git branch ────────────────────────────────────────────────────────────────

/// Runs `git rev-parse --abbrev-ref HEAD` in `workspace_path`.
/// Works for every editor because it operates on the filesystem, not the app.
/// Returns `None` if the path is not inside a git repo or git is unavailable.
pub fn get_git_branch(workspace_path: &str) -> Option<String> {
    if workspace_path.is_empty() {
        return None;
    }
    let path = Path::new(workspace_path);
    // Only run git for paths that look like filesystem roots, not bare names.
    if !path.is_absolute() && !workspace_path.contains('/') {
        return None;
    }
    let out = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;
    if out.status.success() {
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if branch.is_empty() || branch == "HEAD" { None } else { Some(branch) }
    } else {
        None
    }
}

// ── Language inference ────────────────────────────────────────────────────────

/// Infers a human-readable language name from a file name's extension.
pub fn language_from_filename(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs"                        => "Rust",
        "ts" | "tsx"                => "TypeScript",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "py"                        => "Python",
        "r" | "rmd" | "qmd"         => "R",
        "java"                      => "Java",
        "kt" | "kts"                => "Kotlin",
        "swift"                     => "Swift",
        "go"                        => "Go",
        "c" | "h"                   => "C",
        "cpp" | "cxx" | "cc" | "hpp" => "C++",
        "cs"                        => "C#",
        "rb"                        => "Ruby",
        "php"                       => "PHP",
        "scala"                     => "Scala",
        "clj" | "cljs"              => "Clojure",
        "ex" | "exs"                => "Elixir",
        "hs"                        => "Haskell",
        "lua"                       => "Lua",
        "sh" | "bash" | "zsh"       => "Shell",
        "sql"                       => "SQL",
        "html" | "htm"              => "HTML",
        "css" | "scss" | "sass"     => "CSS",
        "json"                      => "JSON",
        "yaml" | "yml"              => "YAML",
        "toml"                      => "TOML",
        "md" | "mdx"                => "Markdown",
        "ipynb"                     => "Jupyter Notebook",
        "m" | "mm"                  => "Objective-C",
        "dart"                      => "Dart",
        "zig"                       => "Zig",
        "vue"                       => "Vue",
        "svelte"                    => "Svelte",
        _                           => return None,
    })
}
