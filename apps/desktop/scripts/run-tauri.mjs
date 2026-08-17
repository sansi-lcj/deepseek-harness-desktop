#!/usr/bin/env node
/**
 * Tool runner for the desktop package. Resolves `tauri`/`vite` from this
 * package's node_modules and exports a deterministic environment for both:
 *
 * - PATH gains the mise shims directory (Rust via mise) and the directory of
 *   the Node.js executable, so `tauri dev` finds `cargo` and the shell finds
 *   `node` even from PATH-starved contexts.
 * - DSH_DESKTOP_NODE pins the Node.js executable the Rust shell should use.
 * - DSH_DESKTOP_REPO_ROOT / DSH_DESKTOP_SERVER_BIN pin the repository checkout
 *   and the built dsh server entry the shell boots.
 * @module run-tauri
 */

import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = path.resolve(desktopRoot, '..')

const [tool, ...args] = process.argv.slice(2)
if (!tool) {
  console.error("[dsh-desktop] usage: run-tauri.mjs <tauri|vite> [args...]")
  process.exit(2)
}

const bin = path.join(
  desktopRoot,
  'node_modules',
  '.bin',
  process.platform === 'win32' ? `${tool}.cmd` : tool,
)
if (!existsSync(bin)) {
  console.error("[dsh-desktop] missing " + bin + "; run `pnpm install` first")
  process.exit(1)
}

const pathEntries = process.env.PATH ? process.env.PATH.split(path.delimiter) : []
const shims = path.join(os.homedir(), '.local/share/mise/shims')
if (existsSync(shims) && !pathEntries.includes(shims)) pathEntries.unshift(shims)
const nodeDir = path.dirname(process.execPath)
if (!pathEntries.includes(nodeDir)) pathEntries.unshift(nodeDir)

const env = {
  ...process.env,
  PATH: pathEntries.join(path.delimiter),
  DSH_DESKTOP_NODE: process.execPath,
  DSH_DESKTOP_REPO_ROOT: repoRoot,
  DSH_DESKTOP_SERVER_BIN: path.join(repoRoot, 'apps', 'cli', 'lib', 'bin.js'),
}

const child = spawn(bin, args, { stdio: 'inherit', env, shell: process.platform === 'win32' })
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal))
}
child.on('error', (error) => {
  console.error("[dsh-desktop] failed to run " + tool + ":", error)
  process.exit(1)
})
child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal)
  process.exit(code ?? 0)
})
