#!/usr/bin/env node
/**
 * Local pre-flight for the desktop CI pipeline. Runs the same build chain
 * the GitHub Actions composite performs, plus static checks on the
 * workflow files, so failures surface on this machine instead of a
 * 20-minute CI round trip. Bundling (dmg/nsis/deb) stays CI-only.
 * @module preflight
 */

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const desktopRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const repoRoot = path.resolve(desktopRoot, '../..')
const require = createRequire(import.meta.url)

function run(cmd, args, opts = {}) {
  const result = spawnSync(cmd, args, { stdio: 'inherit', ...opts })
  if (result.status !== 0) {
    throw new Error(`${cmd} ${args.join(' ')} failed with status ${result.status}`)
  }
}

function lintWorkflows() {
  const files = [
    '.github/workflows/desktop.yml',
    '.github/workflows/desktop-release.yml',
    '.github/actions/build-desktop/action.yml',
  ]
  for (const rel of files) {
    const full = path.join(repoRoot, rel)
    if (!existsSync(full)) {
      throw new Error(`missing ${rel}`)
    }
    // js-yaml ships in the repository root node_modules (upstream deps).
    let yaml
    try {
      yaml = require('js-yaml')
    } catch {
      console.warn(`[preflight] js-yaml unavailable; skipping YAML lint of ${rel}`)
      continue
    }
    yaml.load(readFileSync(full, 'utf8'))
    console.log(`[preflight] yaml ok: ${rel}`)
  }
}

function lintActionScripts() {
  // bash -n the composite action's run blocks, mirroring the CI shell.
  const action = readFileSync(path.join(repoRoot, '.github/actions/build-desktop/action.yml'), 'utf8')
  const blocks = [...action.matchAll(/run: \|\n([\s\S]*?)(?=\n\s{4}\S|\n\s*- )/g)]
    .map((m) => m[1])
    .filter((block) => block.includes('$') || block.includes(';') || block.includes('while'))
  for (const [index, block] of blocks.entries()) {
    const result = spawnSync('bash', ['-n'], { input: block, encoding: 'utf8' })
    if (result.status !== 0) {
      console.error(`[preflight] bash -n failed for run block #${index + 1}:\n${result.stderr}`)
      throw new Error(`syntax error in composite run block #${index + 1}`)
    }
  }
  console.log(`[preflight] bash syntax ok: ${blocks.length} run blocks`)
}

function buildChain() {
  const npmBin = process.platform === 'win32' ? 'npm.cmd' : 'npm'
  console.log('[preflight] installing desktop dependencies')
  run(npmBin, ['ci', '--workspaces=false', '--no-audit', '--no-fund'], { cwd: desktopRoot })
  console.log('[preflight] building splash frontend')
  run(path.join(desktopRoot, 'node_modules/.bin/vite'), ['build', 'frontend'], { cwd: desktopRoot })
  console.log('[preflight] staging bundled server resources')
  run(process.execPath, [path.join(desktopRoot, 'scripts/build-resources.mjs')], { cwd: repoRoot })
  console.log('[preflight] compiling the shell (release)')
  run('cargo', ['build', '--release'], { cwd: path.join(desktopRoot, 'src-tauri') })
}

function crossCheck() {
  const targets = process.argv.includes('--cross') ? ['x86_64-apple-darwin'] : []
  for (const target of targets) {
    console.log(`[preflight] cargo check --target ${target}`)
    run('rustup', ['target', 'add', target])
    run('cargo', ['check', '--release', '--target', target], { cwd: path.join(desktopRoot, 'src-tauri') })
  }
}

lintWorkflows()
lintActionScripts()
buildChain()
crossCheck()
console.log('[preflight] all local checks passed')
