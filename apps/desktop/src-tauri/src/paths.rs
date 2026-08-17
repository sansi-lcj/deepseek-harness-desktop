//! Resolution of the Node.js runtime and the dsh server entry point.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// A runnable dsh server: its Node runtime, entry point, and working directory.
pub struct ResolvedServer {
    /// Node.js executable used to run the server entry.
    pub node: PathBuf,
    /// The built server entry (`bin.js`).
    pub server_bin: PathBuf,
    /// Working directory for the server process; set for the bundled layout.
    pub cwd: Option<PathBuf>,
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

/// Resolve node and server paths: explicit environment first, then the
/// checkout (repo mode), then the resources bundled into the app bundle.
pub fn resolve(app: &AppHandle) -> Result<ResolvedServer, String> {
    let resource_dir = app.path().resource_dir().ok();

    let node = explicit_node()
        .or_else(path_node)
        .or_else(|| {
            resource_dir
                .as_ref()
                .map(|dir| {
                    dir.join(if cfg!(windows) {
                        "resources/node/node.exe"
                    } else {
                        "resources/node/node"
                    })
                })
                .filter(|candidate| is_file(candidate))
        })
        .or_else(|| node_install_candidates().into_iter().find(|candidate| is_file(candidate)));
    let Some(node) = node else {
        return Err(String::from(
            "No Node.js runtime found. Install Node.js and retry, or set DSH_DESKTOP_NODE to its executable path.",
        ));
    };

    // Repo mode: an explicit checkout's built entry wins over the bundle.
    let explicit = explicit_server_bin()
        .or_else(repo_root_server_bin)
        .or_else(|| find_repo_root().map(|root| root.join("apps/cli/lib/bin.js")));
    let bundled = resource_dir
        .as_ref()
        .map(|dir| dir.join("resources/server/node_modules/@deepseek-ai/dsh/lib/bin.js"))
        .filter(|candidate| candidate.is_file());

    // The bundled server runs with its working directory OUTSIDE the app
    // bundle: replacing the .app during an update must not invalidate the
    // running process's cwd.
    let runtime_cwd = app
        .path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join("server-runtime"));
    let (server_bin, cwd) = match (explicit, bundled) {
        (Some(bin), _) => (bin, None),
        (None, Some(bin)) => (bin, runtime_cwd),
        (None, None) => {
            return Err(String::from(
                "The dsh server is unavailable: neither the bundled copy nor a built checkout (apps/cli/lib/bin.js) was found. Run `pnpm run build` or package the app with build-resources first.",
            ));
        }
    };
    Ok(ResolvedServer {
        node,
        server_bin,
        cwd,
    })
}

/// `DSH_DESKTOP_NODE`, when it names an existing file.
fn explicit_node() -> Option<PathBuf> {
    let candidate = PathBuf::from(env::var("DSH_DESKTOP_NODE").ok()?);
    is_file(&candidate).then_some(candidate)
}

/// The first `node` found on PATH.
fn path_node() -> Option<PathBuf> {
    for dir in env::split_paths(&env::var("PATH").ok()?) {
        let candidate = dir.join("node");
        if is_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Well-known Node.js locations: mise and nvm versioned installs, then the
/// Homebrew and system paths.
fn node_install_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        let roots = [
            home.join(".local/share/mise/installs/node"),
            home.join(".nvm/versions/node"),
        ];
        for root in roots {
            let Ok(entries) = fs::read_dir(&root) else {
                continue;
            };
            for entry in entries.flatten() {
                candidates.push(entry.path().join("bin").join("node"));
            }
        }
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/node"));
    candidates.push(PathBuf::from("/usr/local/bin/node"));
    candidates.push(PathBuf::from("/usr/bin/node"));
    candidates
}

/// `DSH_DESKTOP_SERVER_BIN`, when it names an existing file.
fn explicit_server_bin() -> Option<PathBuf> {
    let candidate = PathBuf::from(env::var("DSH_DESKTOP_SERVER_BIN").ok()?);
    candidate.is_file().then_some(candidate)
}

/// `DSH_DESKTOP_REPO_ROOT` plus the built server entry, when both exist.
fn repo_root_server_bin() -> Option<PathBuf> {
    let candidate = PathBuf::from(env::var("DSH_DESKTOP_REPO_ROOT").ok()?).join("apps/cli/lib/bin.js");
    candidate.is_file().then_some(candidate)
}

/// Walk ancestors of the executable and the cwd looking for a checkout.
fn find_repo_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(exe) = env::current_exe() {
        starts.push(exe);
    }
    if let Ok(cwd) = env::current_dir() {
        starts.push(cwd);
    }
    for start in starts {
        let mut dir: &Path = if start.is_dir() {
            &start
        } else {
            start.parent()?
        };
        for _ in 0..8 {
            if dir.join("apps/cli/lib/bin.js").is_file() {
                return Some(dir.to_path_buf());
            }
            dir = dir.parent()?;
        }
    }
    None
}
