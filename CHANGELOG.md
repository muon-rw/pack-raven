# Changelog

We record notable changes to packraven here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and packraven uses [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[Unreleased]: https://github.com/muon-rw/pack-raven/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/muon-rw/pack-raven/releases/tag/v0.1.0
