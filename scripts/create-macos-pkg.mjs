import { existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import { basename, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const repoRoot = resolve(fileURLToPath(new URL('..', import.meta.url)))
const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'))

const packageJson = readJson(join(repoRoot, 'package.json'))
const tauriConfig = readJson(join(repoRoot, 'src-tauri', 'tauri.conf.json'))
const version = packageJson.version
const productName = tauriConfig.productName
const identifier = tauriConfig.identifier
const arch = process.arch === 'arm64' ? 'aarch64' : process.arch
const bundleRoot = join(repoRoot, 'src-tauri', 'target', 'release', 'bundle', 'macos')
const distRoot = join(repoRoot, 'dist')
const targetAppPath = join(bundleRoot, `${productName}.app`)
const distAppPath = join(repoRoot, 'dist', `${productName}.app`)
const appPath = existsSync(targetAppPath) ? targetAppPath : distAppPath
const pkgPath = join(distRoot, `${productName}_${version}_${arch}.pkg`)
const stagingRoot = join(repoRoot, '.tmp', 'macos-pkg-root')
const stagedApplications = join(stagingRoot, 'Applications')
const stagedAppPath = join(stagedApplications, `${productName}.app`)

if (!existsSync(appPath)) {
  throw new Error(`Missing macOS app bundle: ${appPath}`)
}

mkdirSync(distRoot, { recursive: true })
rmSync(stagingRoot, { recursive: true, force: true })
rmSync(pkgPath, { recursive: true, force: true })
mkdirSync(stagedApplications, { recursive: true })

const stageApp = spawnSync('ditto', [
  '--norsrc',
  '--noextattr',
  '--noacl',
  appPath,
  stagedAppPath,
], {
  env: {
    ...process.env,
    COPYFILE_DISABLE: '1',
    DITTONORSRC: '1',
  },
  stdio: 'inherit',
})

if (stageApp.error) {
  throw stageApp.error
}

if (stageApp.status !== 0) {
  throw new Error(`ditto staging failed with exit code ${stageApp.status}`)
}

const result = spawnSync('pkgbuild', [
  '--root',
  stagingRoot,
  '--install-location',
  '/',
  '--identifier',
  identifier,
  '--version',
  version,
  pkgPath,
], {
  env: {
    ...process.env,
    COPYFILE_DISABLE: '1',
  },
  stdio: 'inherit',
})

if (result.error) {
  throw result.error
}

if (result.status !== 0) {
  throw new Error(`pkgbuild failed with exit code ${result.status}`)
}

rmSync(stagingRoot, { recursive: true, force: true })
console.log(`Created ${basename(pkgPath)}`)
