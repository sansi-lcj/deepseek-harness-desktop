#!/usr/bin/env node
/**
 * Assembles the self-contained server payload bundled into the desktop app:
 *
 * - resources/server: the published @deepseek-ai/dsh plus its full
 *   dependency closure, installed by pnpm in a staging directory OUTSIDE the
 *   workspace (inside the repo tree, pnpm would resolve the workspace links
 *   and fail on the `workspace:` protocol), then copied in verbatim.
 * - resources/node: the Node.js executable that runs the server, so the
 *   packaged app needs nothing preinstalled on the target machine.
 *
 * The staged install mirrors the workspace install of the same packages: the
 * staging package.json carries the same node-pty patch, the same build-script
 * allowlist (node-pty, koffi, dsh-subprocess-local), and the @smithy/core pin.
 * The hoisted node linker flattens node_modules like npm does, so the copied
 * tree contains real files only — the bundle copiers in the .app, deb, and
 * NSIS bundlers skip or reject symlinks in resources.
 *
 * Run by tauri's beforeBuildCommand; also runnable standalone.
 * @module build-resources
 */

import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const DSH_VERSION = process.env.DSH_DESKTOP_SERVER_VERSION ?? '0.1.0-rc.6'
const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = path.resolve(desktopRoot, '..', '..')
const resources = path.join(desktopRoot, 'src-tauri', 'resources')
const serverDir = path.join(resources, 'server')
const nodeDir = path.join(resources, 'node')
const stage = path.join(os.tmpdir(), `dsh-server-stage-${process.pid}`)

const NODE_VERSION = '24.19.0'
const NODE_ARCH = process.env.DSH_DESKTOP_NODE_ARCH || `${process.platform}-${process.arch}`

// Node.js archive names use `win`/`darwin`/`linux` instead of node's
// process.platform values (`win32`).
function nodeDistPlatform() {
  return NODE_ARCH.startsWith('win') ? 'win' : NODE_ARCH.split('-')[0]
}

/**
 * Stage the Node.js runtime for the TARGET architecture. Native builds copy
 * the current executable; cross-architecture builds download the matching
 * distribution from nodejs.org.
 */
function stageNode(nodeDir) {
  const isWindows = NODE_ARCH.startsWith('win')
  const nodeName = isWindows ? 'node.exe' : 'node'
  const native = `${process.platform}-${process.arch}`
  if (NODE_ARCH === native || (isWindows && native === 'win32-x64' && NODE_ARCH === 'win-x64')) {
    cpSync(process.execPath, path.join(nodeDir, nodeName))
    return
  }
  const distPlatform = nodeDistPlatform()
  const base = `https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-${NODE_ARCH}`
  const archive = isWindows ? `${base}.zip` : `${base}.tar.gz`
  const archivePath = path.join(stage, path.basename(archive))
  run('curl', ['-fsSL', archive, '-o', archivePath])
  // bsdtar on Windows misreads drive-letter paths as remote hosts; forward
  // slashes keep the -C target local.
  const tarStage = process.platform === 'win32' ? stage.replace(/\\/g, '/') : stage
  const tarArchive = process.platform === 'win32' ? archivePath.replace(/\\/g, '/') : archivePath
  run('tar', ['-xf', tarArchive, '-C', tarStage])
  const extracted = path.join(stage, `node-v${NODE_VERSION}-${NODE_ARCH}`)
  const nodeBin = isWindows
    ? path.join(extracted, 'node.exe')
    : path.join(extracted, 'bin', 'node')
  if (!existsSync(nodeBin)) {
    throw new Error(`downloaded Node.js distribution has no ${nodeBin}`)
  }
  cpSync(nodeBin, path.join(nodeDir, nodeName))
  console.log(`[build-resources] staged node ${NODE_ARCH} for ${distPlatform} target`)
}

/** Recursively delete every `.bin` directory under root. */
function removeBinDirs(root) {
  const stack = [root]
  while (stack.length > 0) {
    const dir = stack.pop()
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue
      const full = path.join(dir, entry.name)
      if (entry.name === '.bin') rmSync(full, { recursive: true, force: true })
      else stack.push(full)
    }
  }
}

function run(cmd, args, opts) {
  let result
  if (process.platform === 'win32') {
    // Node cannot spawn .cmd shims directly; route through the shell.
    const command = [cmd, ...args.map((arg) => (/[\s"]/.test(arg) ? `"${arg}"` : arg))].join(' ')
    result = spawnSync(command, { stdio: 'inherit', shell: true, ...opts })
  } else {
    result = spawnSync(cmd, args, { stdio: 'inherit', ...opts })
  }
  if (result.status !== 0) {
    throw new Error(`${cmd} ${args.join(' ')} failed with status ${result.status}`)
  }
}

rmSync(serverDir, { recursive: true, force: true })
rmSync(nodeDir, { recursive: true, force: true })
rmSync(stage, { recursive: true, force: true })
mkdirSync(stage, { recursive: true })

try {
  writeFileSync(
    path.join(stage, 'package.json'),
    JSON.stringify({ name: 'dsh-server-stage', private: true }, null, 2) + '\n',
  )
  // pnpm reads build-script policy, patches, and overrides from the
  // workspace file (not from package.json), and pnpm 11 hard-fails on any
  // unlisted build script. Mirror the repository's own policy so the staged
  // server behaves like the dev install: allow the native packages we need,
  // deny the reviewed no-op scripts, apply the node-pty spawn-helper patch,
  // and pin @smithy/core (a transitive release once asked for the
  // unpublished ^3.33.1; the pin keeps resolution independent of registry
  // timing).
  writeFileSync(
    path.join(stage, 'pnpm-workspace.yaml'),
    [
      'packages: []',
      'allowBuilds:',
      "  'node-pty': true",
      '  koffi: true',
      "  '@deepseek-ai/dsh-subprocess-local': true",
      "  '@google/genai': false",
      '  protobufjs: false',
      'patchedDependencies:',
      "  'node-pty@1.1.0': patches/node-pty@1.1.0.patch",
      'overrides:',
      "  '@smithy/core': 3.33.0",
    ].join('\n') + '\n',
  )
  mkdirSync(path.join(stage, 'patches'), { recursive: true })
  cpSync(
    path.join(repoRoot, 'patches', 'node-pty@1.1.0.patch'),
    path.join(stage, 'patches', 'node-pty@1.1.0.patch'),
  )
  const pnpmBin = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'
  run(
    pnpmBin,
    ['install', '--prod', '--config.node-linker=hoisted', `@deepseek-ai/dsh@${DSH_VERSION}`],
    { cwd: stage },
  )
  mkdirSync(resources, { recursive: true })
  cpSync(stage, serverDir, { recursive: true })
  // pnpm's .bin entries (top-level and nested under conflicting packages)
  // are absolute symlinks into the staging directory, which is deleted
  // right after this copy; the shell resolves the server entry directly, so
  // every .bin directory is removed wholesale.
  removeBinDirs(path.join(serverDir, 'node_modules'))
  mkdirSync(nodeDir, { recursive: true })
  stageNode(nodeDir)
  if (process.platform !== 'win32') {
    // node-pty's tarball ships every platform's spawn-helper without the
    // exec bit, and the dsh-subprocess-local postinstall restores it only
    // for the install host's arch; a cross build (e.g. x64 on an arm64
    // runner) must restore the foreign-arch helper too.
    run('find', [serverDir, '-name', 'spawn-helper', '-exec', 'chmod', '+x', '{}', ';'])
  }
  console.log(`[build-resources] dsh ${DSH_VERSION} + node staged into ${resources}`)
} finally {
  rmSync(stage, { recursive: true, force: true })
}
