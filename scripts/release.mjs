// Run `npm run release -- 0.2.0` after you add a `## [0.2.0] - YYYY-MM-DD` section to CHANGELOG.md.
// Leave out the version and we ask for it, offering the newest release in the changelog.
// We bump the version, commit, tag and push, and the tag starts .github/workflows/release.yml.
// That section becomes the tag message, which the workflow publishes as the release notes.
// Add --dry-run to commit and tag without pushing.
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { createInterface } from 'node:readline/promises'

const REPO_URL = 'https://github.com/muon-rw/packraven'
const CHANGELOG = 'CHANGELOG.md'
const CARGO_TOML = 'src-tauri/Cargo.toml'
const CARGO_LOCK = 'src-tauri/Cargo.lock'

const args = process.argv.slice(2)
const dryRun = args.includes('--dry-run')
const [givenVersion] = args.filter((arg) => arg !== '--dry-run')

const git = (...gitArgs) => {
  try {
    return execFileSync('git', gitArgs, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
  } catch (error) {
    throw new Error(`git ${gitArgs[0]} failed: ${error.stderr?.trim() || error.message}`)
  }
}
const fail = (text) => {
  console.error(text)
  process.exit(1)
}
// Git and file errors end the run with their message alone, since a stack trace adds nothing for you.
process.on('uncaughtException', (error) => fail(error.message))
process.on('unhandledRejection', (error) => fail(error instanceof Error ? error.message : String(error)))

const changelog = readFileSync(CHANGELOG, 'utf8').replace(/\r\n/g, '\n')
const lines = changelog.split('\n')
const heading = /^## \[([^\]]+)\](.*)$/
const headings = lines.flatMap((line, index) => {
  const match = line.match(heading)
  return match ? [{ index, name: match[1], rest: match[2].trim() }] : []
})
const releaseHeadings = headings.filter((h) => h.name.toLowerCase() !== 'unreleased')

const askForVersion = async () => {
  const newest = releaseHeadings[0]?.name
  const prompt = createInterface({ input: process.stdin, output: process.stdout })
  const answer = (await prompt.question(`Version to release${newest ? ` (Enter for ${newest})` : ''}: `)).trim()
  prompt.close()
  return answer || newest
}
const version = givenVersion ?? (await askForVersion())
if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) fail(`Release versions look like 1.2.3, not "${version ?? ''}". Usage: npm run release -- [major.minor.patch] [--dry-run]`)
const tag = `v${version}`

if (git('rev-parse', '--abbrev-ref', 'HEAD') !== 'master') fail('Switch to master first.')
if (git('status', '--porcelain')) fail('Commit or stash your changes first.')
git('fetch', 'origin', 'master', '--tags')
if (git('rev-parse', 'HEAD') !== git('rev-parse', 'origin/master')) fail('Your master differs from origin/master. Pull or push first.')
if (git('tag', '--list', tag)) fail(`${tag} already exists.`)

const lastTag = (() => {
  try {
    return git('describe', '--tags', '--abbrev=0', '--match', 'v*')
  } catch {
    return null
  }
})()
const compareVersions = (a, b) => {
  const [x, y] = [a, b].map((v) => v.split('.').map(Number))
  return x.map((part, i) => part - y[i]).find((difference) => difference !== 0) ?? 0
}
if (lastTag && compareVersions(version, lastTag.slice(1)) <= 0) fail(`${version} isn't newer than the last release, ${lastTag}.`)

const section = releaseHeadings.find((h) => h.name === version)
// The en-CA locale formats dates as YYYY-MM-DD, in local time rather than toISOString's UTC.
const today = new Date().toLocaleDateString('en-CA')
if (!section) fail(`Add a "## [${version}]" section to ${CHANGELOG}, with this release's notes.`)
if (releaseHeadings[0] !== section) fail(`[${version}] has to be the newest release in ${CHANGELOG}, above [${releaseHeadings[0].name}].`)
if (!section.rest) lines[section.index] = `## [${version}] - ${today}`
else if (!/^- \d{4}-\d{2}-\d{2}$/.test(section.rest)) fail(`Date the [${version}] heading in ${CHANGELOG} like "## [${version}] - ${today}", or leave the date off and we'll add today's.`)

const reference = /^\[([^\]]+)\]: /
const sectionLines = (start) => {
  const body = []
  for (const line of lines.slice(start + 1)) {
    if (heading.test(line) || reference.test(line)) break
    body.push(line)
  }
  return body
}
const notes = sectionLines(section.index).join('\n').trim()
if (!notes) fail(`The [${version}] section in ${CHANGELOG} is empty.`)

const unreleased = headings.find((h) => h.name.toLowerCase() === 'unreleased')
if (unreleased && sectionLines(unreleased.index).some((line) => line.startsWith('- '))) {
  console.warn(`Unreleased in ${CHANGELOG} still has entries, so they won't be in ${tag}.`)
}

// Keep a Changelog links each heading to its diff with reference links at the end of the file,
// newest first, so we put this release's links above the older ones.
const ours = [`[Unreleased]: ${REPO_URL}/compare/${tag}...HEAD`, `[${version}]: ${lastTag ? `${REPO_URL}/compare/${lastTag}...${tag}` : `${REPO_URL}/releases/tag/${tag}`}`]
const kept = lines.filter((line) => !['unreleased', version].includes(line.match(reference)?.[1].toLowerCase()))
while (kept.at(-1) === '') kept.pop()
const firstReference = kept.findIndex((line) => reference.test(line))
if (firstReference === -1) kept.push('', ...ours)
else kept.splice(firstReference, 0, ...ours)
writeFileSync(CHANGELOG, `${kept.join('\n')}\n`)

const packageVersion = /^(\[package\][^[]*?\nversion = ")([^"]+)(")/m
const lockVersion = /(\nname = "packraven"\r?\nversion = ")([^"]+)(")/
const toml = readFileSync(CARGO_TOML, 'utf8')
const lock = readFileSync(CARGO_LOCK, 'utf8')
if (!packageVersion.test(toml)) fail(`Couldn't find the package version in ${CARGO_TOML}.`)
if (!lockVersion.test(lock)) fail(`Couldn't find packraven in ${CARGO_LOCK}.`)
writeFileSync(CARGO_TOML, toml.replace(packageVersion, `$1${version}$3`))
writeFileSync(CARGO_LOCK, lock.replace(lockVersion, `$1${version}$3`))

git('commit', '--quiet', '-m', `Release ${version}`, CHANGELOG, CARGO_TOML, CARGO_LOCK)
// Verbatim cleanup keeps the "### Added" style headings, which git would otherwise strip as comments.
git('tag', '--annotate', tag, '--cleanup=verbatim', '-m', notes)
console.log(`Committed and tagged ${tag} with these notes:\n\n${notes}\n`)

if (dryRun) {
  console.log(`Dry run, so we didn't push. To undo: git tag -d ${tag} && git reset --hard HEAD~1`)
} else {
  git('push', '--atomic', 'origin', 'master', `refs/tags/${tag}`)
  console.log(`Pushed. Follow the build at ${REPO_URL}/actions`)
}
