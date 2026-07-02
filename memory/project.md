# Project V.E.L.O.C.I.T.Y. OS

V.E.L.O.C.I.T.Y. is a self-hosting agent workspace that empowers the agent to build and iterate on its own IDE from within.

## Architecture

- **Harness**: `agent/main.py` is the execution driver.
- **Tools**: `agent/tools.py` provides core shell, git, search, and filesystem access.
- **Config**: Cloudflare Workers AI integration for API completions.
- **Workspace**: Everything is encapsulated, run inside a container via Podman.

## Developer Info
- **Developer Name**: UnitBuilds
- **Developer Email**: ian@unitbuilds.com
- **Repository**: https://github.com/UnitBuilds/Kimi-Code
