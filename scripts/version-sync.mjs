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

/// Front-end HTML files where the Settings → Version element lives.
/// Each file must contain a `<span id="settings-version-value">…</span>`
/// whose text content is empty / a placeholder dash. Anything that
/// looks like a semver literal inside that span is a regression — the
/// row is supposed to be painted at runtime from the Vite-injected
/// `__CODEX_APP_VERSION__`. Restricting the scan to that one element
/// avoids false positives on unrelated version-shaped strings (release
/// links, file paths, dates) that may appear elsewhere in the markup.
const versionHardcodeForbiddenPaths = [
  'src-tauri/mac/front/index.html',
  'src-tauri/win/front/index.html',
]
const settingsVersionElementPattern =
  /<span\b[^>]*\bid\s*=\s*"settings-version-value"[^>]*>([^<]*)<\/span>/i
const versionHardcodePattern = /\b\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?\b/

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
      // Treat a missing path as a configuration drift — silently
      // skipping it would let someone delete or rename the HTML file
      // and lose the safety net unnoticed.
      findings.push({ path: relPath, reason: 'file not found' })
      continue
    }
    const contents = readFileSync(absPath, 'utf8')
    const elementMatch = contents.match(settingsVersionElementPattern)
    if (!elementMatch) {
      findings.push({
        path: relPath,
        reason: 'missing <span id="settings-version-value"> element',
      })
      continue
    }
    const inner = elementMatch[1].trim()
    if (versionHardcodePattern.test(inner)) {
      findings.push({ path: relPath, reason: `inner text "${inner}" looks like a version literal` })
    }
  }
  if (findings.length === 0) {
    return
  }
  const detail = findings
    .map((entry) => `  ${entry.path}: ${entry.reason}`)
    .join('\n')
  throw new Error(
    `Settings → Version row drift detected.\n` +
      `Each listed HTML file must contain <span id="settings-version-value"> whose inner text is a placeholder, not a version literal — the row is painted at runtime from the Vite-injected \`__CODEX_APP_VERSION__\`.\n` +
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
