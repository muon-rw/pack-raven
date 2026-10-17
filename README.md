# packraven

A desktop app for developing and releasing CurseForge modpacks. You keep a CurseForge instance in git without committing mod jars, share it across machines or with collaborators, test it on a local server, and publish the client and server files to CurseForge with a matching git tag.

packraven runs on Windows and macOS. Linux is not yet supported.

## What it does

- **Tracks your modpack in git.** We record every Curseforge file's (mods, etc.) metadata instead of committing them. Anything that isn't on CurseForge is instead added to git directly.
- **Syncs collaborators.** When you pull, we merge mod lists mod by mod and download whatever the other side added. Mods you lock in the CurseForge app stay locked in packraven, and the reverse.
- **Makes your server pack.** We detect client-only mods from Modrinth and CurseForge metadata, so the server pack leaves them out. You can override any mod's side settings as needed.
- **Tests before you ship.** Start an offline test server built from your server pack, and a client that joins it automatically, so you can quickly run end to end tests before publishing. 
- **Publishes.** Set a version number and release notes, we'll detect all mod changes. packraven then uploads the files to Curseforge, attaching the server file to the main upload automatically. It also commits, tags, and (optionally) pushes to git.
- **Announces.** Follow the status of your CurseForge review automatically, then announce the release on Discord via a webhook. Discohook support is built in.
- **Manages mods.** Search CurseForge, add and update mods, see file changelogs, and disable any pack file, in fewer clicks. 
- **Edit Files.** Browse files in your pack, and edit them right in the app. We validate most file formats (json, toml, etc.), can view images and play media. 

packraven sits in the tray and can start when you sign in.

## Getting started

Click **Add pack** and pick one of two paths:

- **Set up a pack** from an existing CurseForge instance. packraven lists every step before it makes a change: it creates a git repository if there isn't one, installs the sync hooks in `.githooks`, writes a `.gitignore` that leaves out logs, saves, caches and mod jars, and writes `mods.lock.json`. It stages the result, and you make the commits yourself.
- **Join a pack** from a git remote. Create a profile in the CurseForge app with the pack's Minecraft version and mod loader, then point packraven at it and the remote. packraven fetches the pack, links the hooks, downloads the mods, and sets aside any CurseForge files in the profile that the pack doesn't include. If the profile's version or loader doesn't match the pack's, packraven stops without changing anything.

If packraven can't find git, it offers to install it (Git for Windows, or the macOS developer tools). The test server and client need Java; packraven uses the same Java you run the game with, or falls back to `java` on your PATH.

### Tokens

Publishing needs a CurseForge upload token, and pushing over HTTPS to GitHub needs a GitHub token. Add them under **App settings**, where you can store each one in the system keychain or name an environment variable to read it from. For older setups, we still read `CURSEFORGE_TOKEN` or `CF_TOKEN` until you choose a source.

### Pack settings

Each pack's settings live in `packraven.toml` at the root of the pack repo, so collaborators share them. Edit them under **Pack settings**: the CurseForge project and release type, files to leave out of the client and server exports, client-only and keep-on-server overrides, server memory and properties, the test server port and username, Discord announcement text, and the update channel.

Release notes go in `changelog.md`. When you publish, we archive them to a file for that version and clear the working notes. We store the pack version in the Better Compatibility Checker config.

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

We embed the key in the binary (masked) at build time, so it never goes in the repo. Without it, packraven can't look up or recognize mods on CurseForge.

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
