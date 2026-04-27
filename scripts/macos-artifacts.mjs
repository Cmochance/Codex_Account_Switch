import {
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  symlinkSync,
} from 'node:fs'
import { basename, dirname, extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)))
const distRoot = join(repoRoot, 'dist')
const historyRoot = join(distRoot, 'history')
const releaseRoot = join(repoRoot, 'src-tauri', 'target', 'release')
const bundleRoot = join(releaseRoot, 'bundle')
const packageJsonPath = join(repoRoot, 'package.json')
const packageVersion = JSON.parse(readFileSync(packageJsonPath, 'utf8')).version
const versionPattern = /\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?/

const args = new Map(
  process.argv
    .slice(2)
    .filter((arg) => arg.startsWith('--') && arg.includes('='))
    .map((arg) => {
      const [key, value] = arg.slice(2).split('=', 2)
      return [key, value]
    }),
)

const phase = args.get('phase') ?? 'finalize'
const mode = args.get('mode') ?? 'release'

if (!['prepare', 'finalize'].includes(phase)) {
  throw new Error(`Unknown macOS artifact phase: ${phase}`)
}

if (!['app', 'release'].includes(mode)) {
  throw new Error(`Unknown macOS artifact mode: ${mode}`)
}

const relativePath = (path) => relative(repoRoot, path)

const lstatPath = (path) => {
  try {
    return lstatSync(path)
  } catch (error) {
    if (error?.code === 'ENOENT') {
      return null
    }

    throw error
  }
}

const listDir = (dir) => existsSync(dir) ? readdirSync(dir).map((name) => join(dir, name)) : []

const ensureDir = (path) => {
  mkdirSync(path, { recursive: true })
}

const removePath = (path) => {
  if (!lstatPath(path)) {
    return
  }

  rmSync(path, { recursive: true, force: true })
  console.log(`Removed ${relativePath(path)}`)
}

const movePath = (sourcePath, destinationDir) => {
  ensureDir(destinationDir)
  const destinationPath = join(destinationDir, basename(sourcePath))
  removePath(destinationPath)
  renameSync(sourcePath, destinationPath)
  console.log(`Archived ${relativePath(sourcePath)} -> ${relativePath(destinationPath)}`)
}

const artifactVersion = (path) => {
  const match = basename(path).match(versionPattern)
  return match?.[0] ?? packageVersion
}

const extension = (path) => extname(basename(path)).toLowerCase()
const isAppBundle = (path) => extension(path) === '.app'
const isInstaller = (path) => extension(path) === '.dmg' || extension(path) === '.pkg'
const isLooseMacExecutable = (path, stats) => {
  const name = basename(path)
  return stats.isFile() && extname(name) === '' && /^codex_switch(?:-\d+)?$/.test(name) && (stats.mode & 0o111) !== 0
}

const ensureBundleOutputLink = (linkPath) => {
  ensureDir(distRoot)
  ensureDir(dirname(linkPath))

  const current = lstatPath(linkPath)
  if (current) {
    removePath(linkPath)
  }

  symlinkSync(relative(dirname(linkPath), distRoot), linkPath, 'dir')
  console.log(`Linked ${relativePath(linkPath)} -> ${relativePath(distRoot)}`)
}

const ensureBundleOutputLinks = () => {
  ensureBundleOutputLink(join(bundleRoot, 'macos'))
  ensureBundleOutputLink(join(bundleRoot, 'dmg'))
}

const archiveOldRootInstallers = () => {
  for (const artifactPath of listDir(distRoot)) {
    if (!isInstaller(artifactPath)) {
      continue
    }

    const version = artifactVersion(artifactPath)
    if (version === packageVersion) {
      removePath(artifactPath)
    } else {
      movePath(artifactPath, join(historyRoot, `v${version}`))
    }
  }
}

const removeDistApps = (dir = distRoot) => {
  if (!existsSync(dir)) {
    return
  }

  for (const path of listDir(dir)) {
    const stats = lstatPath(path)
    if (!stats) {
      continue
    }

    if (isAppBundle(path)) {
      removePath(path)
      continue
    }

    if (stats.isDirectory() && !stats.isSymbolicLink()) {
      removeDistApps(path)
    }
  }
}

const removeLooseExecutablesInDist = (dir = distRoot) => {
  if (!existsSync(dir)) {
    return
  }

  for (const path of listDir(dir)) {
    const stats = statSync(path)
    if (stats.isDirectory()) {
      if (!isAppBundle(path)) {
        removeLooseExecutablesInDist(path)
      }
      continue
    }

    if (isLooseMacExecutable(path, stats)) {
      removePath(path)
    }
  }
}

const removeBuildExecutables = () => {
  removePath(join(releaseRoot, 'codex_switch'))
  removePath(join(repoRoot, 'src-tauri', 'target', 'debug', 'codex_switch'))
}

const currentInstallers = () => listDir(distRoot)
  .filter((path) => isInstaller(path) && artifactVersion(path) === packageVersion)

const assertCurrentReleaseInstallers = () => {
  const installers = currentInstallers().map((path) => basename(path))
  const hasDmg = installers.some((name) => name.endsWith('.dmg'))
  const hasPkg = installers.some((name) => name.endsWith('.pkg'))
  const missing = [
    hasDmg ? null : '.dmg',
    hasPkg ? null : '.pkg',
  ].filter(Boolean)

  if (missing.length > 0) {
    throw new Error(`Missing final macOS installer(s) in dist/: ${missing.join(', ')}`)
  }
}

ensureDir(distRoot)

if (phase === 'prepare') {
  if (mode === 'release') {
    archiveOldRootInstallers()
    removeDistApps()
  }

  ensureBundleOutputLinks()
  removeLooseExecutablesInDist()
  removeBuildExecutables()
} else {
  if (mode === 'release') {
    removeDistApps()
  }

  removeLooseExecutablesInDist()
  removeBuildExecutables()

  if (mode === 'release') {
    assertCurrentReleaseInstallers()
  }
}
