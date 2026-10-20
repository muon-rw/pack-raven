# packraven

https://packraven.app

## Building from source

You need [Node.js](https://nodejs.org/), [Rust](https://rustup.rs/) 1.91 or newer, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your platform.

```sh
npm install
npm run tauri dev
```

For a release build, set `PACKRAVEN_CF_API_KEY` to packraven's CurseForge for Studios API key:

```sh
PACKRAVEN_CF_API_KEY=... npm run tauri build
```

This key is masked and embedded in the binary at build time; it's not in this repo. Without one, packraven won't be able to use many of its Curseforge integrations.
If you create a fork and publish it, do not unmask and embed packraven's API key. You must use your own. 

## Development

| Path | Contents |
| --- | --- |
| `src-tauri/src/` | The Rust backend. All file, git, network and process work happens here. |
| `src/` | The Svelte 5 + TypeScript UI, built with Vite. It reaches the backend only through `src/lib/api.ts`. |
| `src/dev/mock.ts` | A stand-in backend we install when the UI runs in a plain browser, so you can work on the UI without Tauri. |
| `e2e/` | Playwright tests that render the UI against the mock backend. |

Run `npm run dev` and open the printed URL to work on the UI in a browser against the mock. Add a mock handler for any new backend command the UI calls on load, or `npm run test:ui` will fail.

| Command | What it runs |
| --- | --- |
| `npm run check` | Type-checks the UI, the Playwright tests and the configs. |
| `npm run test:ui` | Renders every view in every mock scenario and fails on errors. |
| `npm run shots` | Screenshots every view, scenario, width and theme into `shots/`. |
| `cargo test --manifest-path src-tauri/Cargo.toml` | Backend tests. |
| `npm run test:live` | Backend tests that call CurseForge. These need `PACKRAVEN_CF_API_KEY` set at build time. |

## Releasing

Add each change under `## [Unreleased]` in `CHANGELOG.md` as you make it, following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). To release, move those notes into a section like `## [0.2.0]` (today's date will be added if you leave it off), commit, and run `npm run release -- 0.2.0` (or the **Release** run configuration, which asks for the version) on an up-to-date, clean `master`. We check that the section exists, has notes, is the newest release in the file, and is newer than the last release tag. Then we update the changelog's compare links, bump the version in `src-tauri/Cargo.toml`, commit, tag `v0.2.0` with that section's notes, and push. 

Add `--dry-run` to commit and tag without pushing.

The tag starts `.github/workflows/release.yml`, which builds the Windows installer and a universal macOS app into a draft GitHub release with the tag's notes, signs the update bundles, and publishes the release once both platforms are in `latest.json`. Installed copies of packraven check that file in the newest release for updates.

The workflow needs these repository secrets:

| Secret | What it holds |
| --- | --- |
| `PACKRAVEN_CF_API_KEY` | packraven's CurseForge for Studios API key. |
| `TAURI_SIGNING_PRIVATE_KEY` | The contents of the updater signing key from `npx tauri signer generate`. Its public half goes in `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | That key's password. |

Keep a backup of the signing key. If you lose it, installed copies can't verify any later update, and their users have to reinstall by hand.
