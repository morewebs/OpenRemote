# Example harness plugin

Copy this folder, rename it in `openremote.json`, and replace the driver's
marked sections with your harness. `docs/harness-plugins.md` is the contract;
this template is the shape.

## What is here

```
openremote.json          the manifest, with the harness block
driver/                  a Rust crate that speaks the driver protocol
```

## Build the driver

```sh
cd driver
cargo build --release
```

Copy the binary to the manifest's path for your OS, for example
`bin/windows-x64/example-harness-driver.exe`:

```powershell
mkdir bin\windows-x64
copy driver\target\release\example-harness-driver.exe bin\windows-x64\
```

## Try it

```powershell
echo '{"type":"probe"}' | .\bin\windows-x64\example-harness-driver.exe --probe
echo '{"type":"spawn","cwd":"."}' | .\bin\windows-x64\example-harness-driver.exe
```

## Install it

In OpenRemote, open Plugins, install from a folder, and choose this folder.
The install screen shows that the plugin runs programs; that is what a harness
driver does. After the install the harness appears in New chat, in the harness
manager, and in Plugins under Installed with a Harness chip.

## What to replace

- `probe()` in `driver/src/main.rs`: find your CLI, ask its version, and
  decide whether it is signed in.
- `spawn()` and the `prompt` arm: start your harness, send the prompt with
  whatever wire it has, and emit `text`, `tool_started`, `tool_result` and
  `turn_completed` as it answers.
- `interrupt()`: cancel the turn and emit the interrupted boundary, or refuse
  honestly.
- `models` in `openremote.json` when the harness has a model catalog.
