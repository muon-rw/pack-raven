# Changelog

We record notable changes to packraven here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and packraven uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.3]
### Fixed
- File editor's view pane jumping around to incorrect locations when typing at certain window sizes

## [0.1.2] - 2026-10-20
### Added
- Improved search within log view, adding jump-to and surrounding lines.
- Java settings list the Javas packraven finds on this computer (CurseForge, JAVA_HOME, PATH, installed JDKs, IntelliJ, Gradle, Prism and Modrinth), so you pick one instead of typing a path
- When no Java of a version a pack needs is installed, packraven offers to download Oracle GraalVM for it
- Test runs fall back to any installed Java of the right version when CurseForge hasn't installed one or it wasn't detected
- You can launch and kill the test client from Mods, and start, restart and stop the test server from Server
- Server and Run tell you when the test server stops on its own, such as when a mod crashes it
- If the test server takes too long to stop, Stop server in the Server view turns into Kill server

### Changed
- Renamed the Test view to Run

### Fixed
- Mod version locks set in packraven now properly lock in Curseforge
- Opening or refreshing an instance in CurseForge no longer clears locks you only set in packraven

## [0.1.1] - 2026-10-20

### Added
- Mod updates and installs can be queued while others download
- Queued mod changes can be cancelled (cancelling one row cancels everything you started with it)
- Mods can be added while the test client runs, and load on its next launch

### Changed
- Join server is now unticked by default, and saves your choice when you leave Test
- The previous run's logs are cleared when you start a test server or client
- Test start buttons wait for mod changes to finish
- Unavailable mod controls now say why on hover

### Fixed
- Launching a client without Join server showed the box ticked
- Mods list missed changes from outside packraven (CurseForge app, git pull), hiding update flags. Check for updates now reloads it too
- Rail showed a spinner next to the status dot of a busy view

## [0.1.0] - 2026-10-19

### Added
- packraven can now keep itself up to date
- Mods tab: browse CurseForge and install with dependencies
- Update, roll back, lock, disable and remove mods, one-at-a-time or in bulk
- Review pending updates and their changelogs before Update all
- Resource packs, shaders and data packs can now be disabled
- Mod version locks now sync automatically with the CurseForge app
- Server Mods tab: Renamed to Server, added `server.properties` editor, JVM flag and arg presets
- Client and server publish uploads can now be renamed
- Pack version can now be edited from the rail
- Added Microsoft sign-in for test clients and online test servers. **NOTE: This doesn't work until Mojang approves our API access request. Hopefully soon!** 
- Can now horizontally resize panes in Files, Git and Test

### Changed
- Mods is now the default view
- Test servers now require accepting the Minecraft EULA
- Offline test servers now only accept local connections (for Mojang license compliance)
- Publishing is blocked while two versions of one mod are enabled (CurseForge rejects manifests with these)

### Fixed
- Java detection for CurseForge's new install folders
- Mod updates skip jars Minecraft is currently holding open instead of stopping partway
- Failed downloads will retry up to three times

[Unreleased]: https://github.com/muon-rw/pack-raven/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/muon-rw/pack-raven/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/muon-rw/pack-raven/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/muon-rw/pack-raven/releases/tag/v0.1.0
