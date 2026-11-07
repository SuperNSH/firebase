# Emulator suite

The Firebase Local Emulator Suite for project `demo-nsh` (a `demo-` project
needs no login and no credentials). Start it with `bun run emulator:start`
(scripts/start-emulator.sh); ports: auth 19099, database 19000, storage
19199, hub 14400, UI disabled.

The rules in `database.rules.json` and `storage.rules` are sandbox-only:
fully permissive read/write for the local demo-nsh emulators. They must
never be deployed to a real Firebase project.
