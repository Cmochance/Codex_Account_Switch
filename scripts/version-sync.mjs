import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)))
const packageJsonPath = resolve(repoRoot, 'package.json')
const packageLockPath = resolve(repoRoot, 'package-lock.json')
const tauriConfigPath = resolve(repoRoot, 'src-tauri', 'tauri.conf.json')
const cargoTomlPath = resolve(repoRoot, 'src-tauri', 'Cargo.toml')
const defaultVersionLogPath = resolve(repoRoot, 'src-tauri', 'target', 'release', 'version.md')
const tauriVersionSource = '../package.json'

/// Files where a hardcoded version literal in markup is always a bug —
/// the HTML now relies on Vite-injected `__CODEX_APP_VERSION__` for the
/// settings row, so any `1.x.y` string left here is a regression that
/// will silently drift from `package.json` again. Add new
/// product-facing HTML/JSX files here as the front-end grows so the
/// check stays comprehensive.
const versionHardcodeForbiddenPaths = [
  'src-tauri/mac/front/index.html',
  'src-tauri/win/front/index.html',
]
const versionHardcodePattern = /\b\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?\b/g

const semverPattern = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/

const readJson = (filePath) => JSON.parse(readFileSync(filePath, 'utf8'))

const writeJson = (filePath, value) => {
  writeFileSync(filePath, `${JSON.stringify(value, null, 2)}\n`)
}

const compareNumericIdentifiers = (left, right) => Number(left) - Number(right)

const comparePrereleaseIdentifiers = (left, right) => {
  const leftNumeric = /^\d+$/.test(left)
  const rightNumeric = /^\d+$/.test(right)

  if (leftNumeric && rightNumeric) {
    return compareNumericIdentifiers(left, right)
  }

  if (leftNumeric) {
    return -1
  }

  if (rightNumeric) {
    return 1
  }

  return left.localeCompare(right)
}

const compareVersions = (leftVersion, rightVersion) => {
  const [leftCore, leftPrerelease] = leftVersion.split('-', 2)
  const [rightCore, rightPrerelease] = rightVersion.split('-', 2)
  const leftCoreParts = leftCore.split('.').map(Number)
  const rightCoreParts = rightCore.split('.').map(Number)

  for (let index = 0; index < Math.max(leftCoreParts.length, rightCoreParts.length); index += 1) {
    const leftPart = leftCoreParts[index] ?? 0
    const rightPart = rightCoreParts[index] ?? 0

    if (leftPart !== rightPart) {
      return leftPart - rightPart
    }
  }

  if (!leftPrerelease && !rightPrerelease) {
    return 0
  }

  if (!leftPrerelease) {
    return 1
  }

  if (!rightPrerelease) {
    return -1
  }

  const leftIdentifiers = leftPrerelease.split('.')
  const rightIdentifiers = rightPrerelease.split('.')

  for (let index = 0; index < Math.max(leftIdentifiers.length, rightIdentifiers.length); index += 1) {
    const leftIdentifier = leftIdentifiers[index]
    const rightIdentifier = rightIdentifiers[index]

    if (leftIdentifier === undefined) {
      return -1
    }

    if (rightIdentifier === undefined) {
      return 1
    }

    const diff = comparePrereleaseIdentifiers(leftIdentifier, rightIdentifier)
    if (diff !== 0) {
      return diff
    }
  }

  return 0
}

const extractLatestVersionFromLog = (filePath) => {
  const content = readFileSync(filePath, 'utf8')
  const versions = [...content.matchAll(/\b\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?\b/g)]
    .map((match) => match[0])

  if (versions.length === 0) {
    throw new Error(`No semver version found in ${filePath}`)
  }

  return versions.reduce((latest, current) => (
    compareVersions(current, latest) > 0 ? current : latest
  ))
}

const syncCargoVersion = (cargoToml, version) => {
  const newline = cargoToml.includes('\r\n') ? '\r\n' : '\n'
  const lines = cargoToml.split(/\r?\n/)
  let inPackageSection = false
  let updated = false

  const nextLines = lines.map((line) => {
    if (line.startsWith('[')) {
      inPackageSection = line === '[package]'
      return line
    }

    if (inPackageSection && line.startsWith('version = ')) {
      updated = true
      inPackageSection = false
      return `version = "${version}"`
    }

    return line
  })

  if (!updated) {
    throw new Error('Failed to locate Cargo package version field.')
  }

  return nextLines.join(newline)
}

const syncPackageLockVersion = (version) => {
  if (!existsSync(packageLockPath)) {
    return
  }

  const packageLock = readJson(packageLockPath)
  let updated = false

  if (packageLock.version !== version) {
    packageLock.version = version
    updated = true
  }

  if (packageLock.packages?.[''] && packageLock.packages[''].version !== version) {
    packageLock.packages[''].version = version
    updated = true
  }

  if (updated) {
    writeJson(packageLockPath, packageLock)
  }
}

const checkHardcodedVersions = () => {
  const findings = []
  for (const relPath of versionHardcodeForbiddenPaths) {
    const absPath = resolve(repoRoot, relPath)
    if (!existsSync(absPath)) {
      continue
    }
    const contents = readFileSync(absPath, 'utf8')
    const matches = [...contents.matchAll(versionHardcodePattern)].map((m) => m[0])
    if (matches.length > 0) {
      findings.push({ path: relPath, matches: [...new Set(matches)] })
    }
  }
  if (findings.length === 0) {
    return
  }
  const detail = findings
    .map((entry) => `  ${entry.path}: ${entry.matches.join(', ')}`)
    .join('\n')
  throw new Error(
    `Hardcoded version literal(s) found in product HTML — these will drift from package.json.\n` +
      `Replace with the Vite-injected \`__CODEX_APP_VERSION__\` (rendered from \`render.ts\`).\n` +
      `Offending files:\n${detail}`,
  )
}

const args = process.argv.slice(2)
const checkMode = args[0] === '--check'
const setMode = args[0] === '--set'
const setFromVersionLogMode = args[0] === '--set-from-version-log'
const requestedVersion = setMode ? args[1] : null
const requestedVersionLogPath = setFromVersionLogMode
  ? resolve(repoRoot, args[1] ?? defaultVersionLogPath)
  : null

if (checkMode) {
  // Read-only mode used by CI / pre-commit hooks. Skips the writes
  // below and only validates that no hardcoded version literal has
  // crept back into the front-end HTML.
  checkHardcodedVersions()
  console.log('Version drift check passed.')
  process.exit(0)
}

if (setMode && setFromVersionLogMode) {
  throw new Error('Choose either --set or --set-from-version-log.')
}

if (setMode && !requestedVersion) {
  throw new Error('Usage: npm run version:set -- <semver>')
}

if (requestedVersion && !semverPattern.test(requestedVersion)) {
  throw new Error(`Invalid semver version: ${requestedVersion}`)
}

const packageJson = readJson(packageJsonPath)
let versionSourceLabel = 'package.json'

if (requestedVersion) {
  packageJson.version = requestedVersion
  writeJson(packageJsonPath, packageJson)
  versionSourceLabel = 'manual'
}

if (setFromVersionLogMode) {
  const latestVersion = extractLatestVersionFromLog(requestedVersionLogPath)
  packageJson.version = latestVersion
  writeJson(packageJsonPath, packageJson)
  versionSourceLabel = requestedVersionLogPath
}

const version = packageJson.version

if (!semverPattern.test(version)) {
  throw new Error(`package.json contains an invalid semver version: ${version}`)
}

syncPackageLockVersion(version)

const tauriConfig = readJson(tauriConfigPath)

if (tauriConfig.version !== tauriVersionSource) {
  tauriConfig.version = tauriVersionSource
  writeJson(tauriConfigPath, tauriConfig)
}

const cargoToml = readFileSync(cargoTomlPath, 'utf8')
const nextCargoToml = syncCargoVersion(cargoToml, version)

if (nextCargoToml !== cargoToml) {
  writeFileSync(cargoTomlPath, nextCargoToml)
}

// Belt-and-braces: every sync run also enforces "no hardcoded version
// in front-end HTML" so a stale literal can't silently rot through
// release after release the way `1.5.0` did.
checkHardcodedVersions()

console.log(`Version source: ${versionSourceLabel} -> ${version}`)
