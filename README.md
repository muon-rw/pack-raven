# packraven

A desktop app for developing and publishing modpacks. Track a CurseForge instance in git without committing mod jars, share it across machines or with collaborators, test it on a local server, and publish the client and server files to CurseForge. 

packraven runs on Windows and macOS. Linux is not yet supported.

## What it does

- **Tracks your modpack in git.** We record every Curseforge file's (mods, etc.) metadata instead of committing them. Anything that isn't on CurseForge is instead added to git directly.
- **Syncs collaborators.** When you pull, we merge mod lists mod by mod and download whatever the other side added. Mods you version-lock in the CurseForge app stay locked in packraven, and the reverse.
- **Makes your server pack.** We detect client-only mods from Modrinth and CurseForge metadata, so the server pack leaves them out. You can override mod inclusions, server.properties, the icon, and Java settings.
- **Test before you ship.** Start a test server built from your server pack, and a client that joins it automatically, so you can quickly run end to end tests before publishing. 
- **Publishes.** Set a version number and release notes, we'll detect all mod changes. packraven then uploads the files to Curseforge, attaching the server file to the main upload automatically. It also commits, tags, and (optionally) pushes to git.
- **Announces.** Follow the status of your CurseForge review automatically, then announce the release on Discord via a webhook. Discohook support is built in.
- **Manages mods.** Search CurseForge, add and update mods, see file changelogs, and disable any pack file, in fewer clicks. 
- **Edit Files.** Browse files in your pack, and edit them right in the app. We validate most file formats (json, toml, etc.), can view images and play media. 

packraven sits in the tray and can start when you sign in.

## Getting started

Click **Add pack** and pick one of two paths:

- **Set up a pack** from an existing CurseForge instance on your machine. 
- **Join a pack** from a git remote. Create a placeholder instance in the Curseforge app, and merge a git-hosted packraven repository into it.

packraven prompts you to install git if you don't have it. 

### Tokens

Publishing needs a CurseForge upload token, and pushing over HTTPS to GitHub needs a GitHub token. Add these under **App settings**. packraven always uses the system keychain, it doesn't store secure credentials on its own. 

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
