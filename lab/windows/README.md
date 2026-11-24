# Windows route measurements (2026-10-13)

The scripts behind the numbers in `docs/windows.md`. Run on the Windows PC
against a throwaway WSL distribution (`AInodeProbe`, imported from Ubuntu Base
24.04.5 on the NVMe drive, `apt install python3-minimal` for these test
clients only) and a copy of the staged Linux engine on the Windows drive
(`AINODE_ENGINE`, default `/mnt/c/Repos/ainode/wslprobe/engine`). The
results are in `docs/windows.md`.

| Script | Side | Measures |
|---|---|---|
| `bridge-latency.py DISTRO USER SOCK` | Windows | one JSON-RPC call to the helper's unix socket: a `wsl.exe` per call (`oneshot.py`) against one persistent relay (`relay.py`) |
| `relay.py SOCK` | WSL | the persistent relay: JSON lines in on stdin, one socket connection per request, the reply line out on stdout |
| `read-speed.py PATH...` | Windows | sequential reads, e.g. `\\wsl.localhost\...` against NTFS (run from PowerShell: Git Bash collapses the leading `\\`) |
| `llama-load.py EXE GGUF` | Windows | `llama-server.exe` start to `/health` 200, then one completion |
| `in-wsl/fetch-test.py MODELDIR SOCK` | WSL | the 350M from the public host into a store on ext4 or NTFS, export, sha256 |
| `in-wsl/fetch-model.py MODELDIR SOCK URI GIB [PEER]` | WSL | any model: fetch (if missing), `exportmodelpath`, times |
| `in-wsl/io.sh` | WSL | the engine started from `/mnt/c`; ext4 against NTFS writes |
| `kill-signal.ps1`, `kill-modeld.ps1` | Windows | what the Linux side gets when the app's `wsl.exe` is terminated |
