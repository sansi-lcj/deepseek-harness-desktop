//! Managed dsh web server: spawn, readiness discovery, navigation, teardown.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl};

use crate::paths;

const MAIN_WINDOW: &str = "main";
/// Port the splash page reports its internal state to; the shell logs it.
const STATUS_PORT: u16 = 32123;
const STATUS_EVENT: &str = "server-status";
/// The readiness line the web bundle prints once the server binds.
/// Keep the scheme: the token right after this prefix must stay a full URL.
const URL_PREFIX: &str = "dsh web: ";
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const STDERR_TAIL_LINES: usize = 30;

/// Lifecycle status the splash surface renders.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "phase", rename_all = "kebab-case")]
pub enum ServerStatus {
    Starting { detail: String },
    Ready { url: String },
    Exited { code: Option<i32>, detail: String },
}

struct Inner {
    app: AppHandle,
    child: Mutex<Option<Child>>,
    pid: Mutex<Option<u32>>,
    stderr_tail: Mutex<VecDeque<String>>,
    exited: AtomicBool,
    shutdown_requested: AtomicBool,
    restarts_left: AtomicU32,
}

/// Owns the dsh server child process for the shell's lifetime.
pub struct ServerManager {
    inner: Arc<Inner>,
}

impl Clone for ServerManager {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl ServerManager {
    /// Spawn the server and begin watching it; the window navigates when ready.
    pub fn start(app: AppHandle) -> Self {
        start_status_listener();
        let manager = Self {
            inner: Arc::new(Inner {
                app,
                child: Mutex::new(None),
                pid: Mutex::new(None),
                stderr_tail: Mutex::new(VecDeque::new()),
                exited: AtomicBool::new(false),
                shutdown_requested: AtomicBool::new(false),
                restarts_left: AtomicU32::new(3),
            }),
        };
        manager.boot();
        manager
    }

    /// Signal the server to stop, then force it; returns once it is down.
    pub fn shutdown(&self) {
        self.inner.shutdown_requested.store(true, Ordering::SeqCst);
        #[cfg(unix)]
        {
            let Some(pid) = *self.inner.pid.lock().unwrap() else {
                return;
            };
            unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            for _ in 0..30 {
                if self.inner.exited.load(Ordering::SeqCst) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        }
        #[cfg(not(unix))]
        {
            let mut guard = self.inner.child.lock().unwrap();
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
            }
        }
    }

    fn emit(&self, status: ServerStatus) {
        let _ = self.inner.app.emit(STATUS_EVENT, status);
    }

    fn fail(&self, detail: String) {
        eprintln!("[dsh-desktop] {detail}");
        self.emit(ServerStatus::Exited {
            code: None,
            detail: detail.clone(),
        });
        if smoke_enabled() {
            std::process::exit(1);
        }
    }

    fn boot(&self) {
        let resolved = match paths::resolve(&self.inner.app) {
            Ok(resolved) => resolved,
            Err(detail) => {
                self.fail(detail);
                return;
            }
        };
        let node = resolved.node;
        let server_bin = resolved.server_bin;
        let mut command = Command::new(&node);
        command
            .arg(&server_bin)
            .args(["web", "--host", "127.0.0.1", "--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = resolved.cwd {
            if let Err(error) = std::fs::create_dir_all(&cwd) {
                eprintln!("[dsh-desktop] failed to create server runtime dir: {error}");
            }
            command.current_dir(cwd);
        }
        if let Some(node_dir) = node.parent() {
            // Prepend the Node directory so shells the harness later spawns
            // still find node under Finder's minimal PATH.
            let current = std::env::var("PATH").unwrap_or_default();
            command.env(
                "PATH",
                format!(
                    "{}{}{}",
                    node_dir.display(),
                    std::path::MAIN_SEPARATOR,
                    current
                ),
            );
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                self.fail(format!(
                    "Failed to start the dsh server using {}: {}",
                    node.display(),
                    error
                ));
                return;
            }
        };
        self.inner.exited.store(false, Ordering::SeqCst);
        let pid = child.id();
        eprintln!(
            "[dsh-desktop] dsh server started pid={} bin={}",
            pid,
            server_bin.display()
        );
        self.emit(ServerStatus::Starting {
            detail: format!("Server process started (pid {pid})."),
        });
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        *self.inner.child.lock().unwrap() = Some(child);
        *self.inner.pid.lock().unwrap() = Some(pid);
        self.watch_stdout(stdout);
        self.watch_stderr(stderr);
        self.watch_exit();
    }

    fn watch_stdout(&self, stdout: impl Read + Send + 'static) {
        let manager = self.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                println!("[dsh] {line}");
                if line.starts_with(URL_PREFIX) {
                    let mut url: String = line[URL_PREFIX.len()..]
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    if !url.contains("://") {
                        url = format!("http://{url}");
                    }
                    if !url.is_empty() {
                        manager.on_server_url(url);
                    }
                }
            }
        });
    }

    fn watch_stderr(&self, stderr: impl Read + Send + 'static) {
        let inner = self.inner.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                eprintln!("[dsh:err] {line}");
                let mut tail = inner.stderr_tail.lock().unwrap();
                if tail.len() >= STDERR_TAIL_LINES {
                    tail.pop_front();
                }
                tail.push_back(line);
            }
        });
    }

    fn watch_exit(&self) {
        let manager = self.clone();
        thread::spawn(move || {
            let status = {
                let mut guard = manager.inner.child.lock().unwrap();
                let Some(mut child) = guard.take() else { return };
                child.wait()
            };
            let Ok(status) = status else { return };
            manager.inner.exited.store(true, Ordering::SeqCst);
            let code = status.code();
            eprintln!("[dsh-desktop] dsh server exited code={code:?}");
            if manager.inner.shutdown_requested.load(Ordering::SeqCst) {
                return;
            }
            let detail = manager
                .inner
                .stderr_tail
                .lock()
                .unwrap()
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            let restarts_left = manager
                .inner
                .restarts_left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
                .unwrap_or(0);
            if restarts_left > 0 {
                eprintln!(
                    "[dsh-desktop] restarting the dsh server ({restarts_left} restarts left)"
                );
                let _ = manager.inner.app.emit(
                    STATUS_EVENT,
                    ServerStatus::Starting {
                        detail: "Restarting the harness server…".to_string(),
                    },
                );
                recover_window(&manager.inner.app, code, &detail, true);
                manager.boot();
            } else {
                let _ = manager.inner.app.emit(
                    STATUS_EVENT,
                    ServerStatus::Exited {
                        code,
                        detail: detail.clone(),
                    },
                );
                recover_window(&manager.inner.app, code, &detail, false);
                if smoke_enabled() {
                    std::process::exit(1);
                }
            }
        });
    }

    fn on_server_url(&self, url: String) {
        eprintln!("[dsh-desktop] server ready: {url}");
        let inner = self.inner.clone();
        let for_poll = url.clone();
        thread::spawn(move || {
            eprintln!("[dsh-desktop] readiness poll for {for_poll}");
            let reachable = match tauri::Url::parse(&for_poll) {
                Ok(parsed) => {
                    let host = parsed.host_str().unwrap_or("127.0.0.1").to_string();
                    let port = parsed.port().unwrap_or(80);
                    wait_until_accepting(&host, port, READY_TIMEOUT)
                }
                Err(_) => true,
            };
            if reachable {
                eprintln!("[dsh-desktop] server accepting connections");
                // The splash page owns the navigation: it listens for this
                // event and moves itself to the server URL, so the readiness
                // path never has to touch the main thread.
                eprintln!("[dsh-desktop] about to emit ready");
                let emit_result = inner.app.emit(
                    STATUS_EVENT,
                    ServerStatus::Ready { url: for_poll.clone() },
                );
                eprintln!("[dsh-desktop] emit returned {emit_result:?}");
                // The dispatcher path talks to the webview process directly and
                // never touches the app main thread.
                if let Some(window) = inner.app.get_webview_window(MAIN_WINDOW) {
                    eprintln!("[dsh-desktop] about to navigate via dispatcher");
                    if let Ok(target) = tauri::Url::parse(&for_poll) {
                        match window.navigate(target) {
                            Ok(()) => eprintln!("[dsh-desktop] navigation issued via dispatcher"),
                            Err(error) => eprintln!("[dsh-desktop] dispatcher navigation failed: {error}"),
                        }
                    }
                } else {
                    eprintln!("[dsh-desktop] main window not found at readiness");
                }
                if let Some(window) = inner.app.get_webview_window(MAIN_WINDOW) {
                    eprintln!(
                        "[dsh-desktop] window url after navigate: {:?}",
                        window.url()
                    );
                }
            } else {
                let detail = format!(
                    "The server printed its URL but did not accept connections within {:?}.",
                    READY_TIMEOUT
                );
                let _ = inner.app.emit(
                    STATUS_EVENT,
                    ServerStatus::Exited { code: None, detail },
                );
            }
        });
    }
}

fn smoke_enabled() -> bool {
    std::env::var("DSH_DESKTOP_SMOKE").is_ok_and(|value| value == "1")
}

/// Logs HTTP state reports the splash page sends, so the shell's stdout
/// carries the page's internal state without needing the event bridge.
fn start_status_listener() {
    thread::spawn(|| {
        let listener = match std::net::TcpListener::bind(("127.0.0.1", STATUS_PORT)) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("[dsh-desktop] status listener bind failed: {error}");
                return;
            }
        };
        eprintln!("[dsh-desktop] status listener on http://127.0.0.1:{STATUS_PORT}");
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 4096];
            let size = stream.read(&mut buffer).unwrap_or(0);
            let text = String::from_utf8_lossy(&buffer[..size]);
            for line in text.lines().take(2) {
                eprintln!("[dsh-splash] {line}");
            }
            let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
        }
    });
}

fn wait_until_accepting(host: &str, port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if std::net::TcpStream::connect((host, port)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(300));
    }
    false
}

/// Rebuild the main window on the splash: an error state for a dead server,
/// or the live restart state when the shell is bringing it back up.
fn recover_window(app: &AppHandle, code: Option<i32>, detail: &str, restarting: bool) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.destroy();
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    if restarting {
        serializer.append_pair("restart", "1");
    } else {
        serializer.append_pair("error", &format!("server exited code={code:?}\n{detail}"));
    }
    let query = serializer.finish();
    let splash = WebviewUrl::App(format!("index.html?{query}").into());
    if let Err(error) = crate::main_window_builder(app, splash).build() {
        eprintln!("[dsh-desktop] failed to rebuild the splash window: {error}");
    }
}
