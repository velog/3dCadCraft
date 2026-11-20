# Connecting an AI agent (MCP)

3DCadCraft serves the [Model Context Protocol](https://modelcontextprotocol.io) over stdio, so
Claude Code, Claude Desktop and other MCP clients can build parts and assemblies with it. What
each tool does is in [spec/0.7-api-mcp.md](spec/0.7-api-mcp.md); the client also reads the
descriptions and input schemas from the server (`tools/list`).

## Build the server

```bash
cargo build --release -p cadcraft3d-core --bin 3dcadcraft-mcp   # headless, no graphics stack
# or use the desktop binary: 3dcadcraft --mcp
```

The binary is `target/release/3dcadcraft-mcp` (`.exe` on Windows). Command line:
`3dcadcraft-mcp [--read-only] [--attach] [PROJECT]` (the same after `3dcadcraft --mcp`):

- `PROJECT` opens a `.3dcadcraft` file before the first message (headless only);
- `--read-only` lists and runs only the tools that read (queries, measure, physical properties,
  export) — for an agent that may look but not change;
- `--attach` connects to the running app instead (below); with no app accepting connections it
  serves headless and says so on stderr.

Every call is written as a line `kind: "ai"` into the session's action journal (the logs folder,
Help › Save Diagnostics), next to what the user did.

## Working in the open app (attach)

To watch the agent work and undo its steps with Ctrl+Z:

1. In 3DCadCraft: Tools › Automation › **AI Connector** (alias `AIC`), tick **Allow AI connections**.
   The status bar shows `AI` (green while a client is connected, `AI: <command>` while a call runs);
   the window shows the commands below with Copy buttons. The setting is kept in Application Options.
2. Configure the client with `--mcp --attach` (the desktop binary, or `3dcadcraft-mcp --attach`).

```bash
claude mcp add 3dcadcraft -- "C:\Program Files\3DCadCraft\3dcadcraft.exe" --mcp --attach
```

Each changing call runs in the background on a copy of the open project and becomes one Undo step
named `AI: <command>` (a batch is one step). While you have a command, a sketch or a rebuild open, the
agent's changing calls are refused with `<Command>: the user is editing (...); try again when it is
closed`; reads still answer. `project_new` / `project_open` are refused (your tabs stay yours);
`project_save` saves your file like Ctrl+S; `undo` / `redo` are the app's own.

The connection is for the current user only: a named pipe (Windows) or a socket file in the settings
folder (Unix), and a random token in `mcp-connector.json` of the settings folder that the bridge sends
first. Turning the option off removes it.

## Claude Code

```bash
claude mcp add 3dcadcraft -- /path/to/3dcadcraft-mcp
# Windows
claude mcp add 3dcadcraft -- C:\path\to\3dcadcraft-mcp.exe
# the open app instead (Tools > AI Connector on), or only looking:
claude mcp add 3dcadcraft -- C:\path\to\3dcadcraft.exe --mcp --attach
claude mcp add 3dcadcraft-view -- C:\path\to\3dcadcraft-mcp.exe --read-only
```

`claude mcp list` shows whether it connected (`✓ Connected`); inside a session `/mcp` lists the
server and its tools.

or in the project's `.mcp.json`:

```json
{
  "mcpServers": {
    "3dcadcraft": {
      "command": "C:\\path\\to\\3dcadcraft-mcp.exe",
      "args": [],
      "env": { "CADCRAFT3D_MCP_ROOT": "C:\\Users\\me\\Documents\\cad" }
    }
  }
}
```

## Claude Desktop

Settings › Developer › Edit Config (`claude_desktop_config.json`), then restart the app:

```json
{
  "mcpServers": {
    "3dcadcraft": {
      "command": "/path/to/3dcadcraft-mcp",
      "env": { "CADCRAFT3D_MCP_ROOT": "/home/me/cad" }
    },
    "3dcadcraft-app": {
      "command": "C:\\path\\to\\3dcadcraft.exe",
      "args": ["--mcp", "--attach"]
    }
  }
}
```

`CADCRAFT3D_MCP_ROOT` is optional: with it the server reads and writes files only inside that
folder. Relative paths resolve against the server's working folder.

## A first conversation

Ask for "a 120 × 80 × 10 mm plate with four M8 clearance holes 20 mm from the edges, saved as
plate.3dcadcraft". The agent calls, in order: `document_new`, `sketch_create {plane: XY}`,
`sketch_rectangle`, `sketch_constrain` (Coincident with the origin, two Distances),
`extrude {distance: 10}`, `hole {face: Top, type: clearance, size: M8, centers: …}`,
`physical`, `project_save`. Open the file in 3DCadCraft to see it; every step is in the browser.

Faces are named persistently (`Extrusion1:End`); `faces` also gives each a short alias (`Top`,
`Extrusion1:+X`) and its direction, and every face argument accepts the alias. `face_at` finds the face
under a point or along a ray.

## Driving it by hand

One JSON-RPC message per line on stdin, answers on stdout:

```text
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"me","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"document_new","arguments":{"kind":"part","name":"Plate"}}}
```

`tools/mcp_drive.py` sends such a script and prints the answers.
