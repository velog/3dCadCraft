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

The binary is `target/release/3dcadcraft-mcp` (`.exe` on Windows). `3dcadcraft-mcp PROJECT`
opens a `.3dcadcraft` file before the first message.

## Claude Code

```bash
claude mcp add 3dcadcraft -- /path/to/3dcadcraft-mcp
# Windows
claude mcp add 3dcadcraft -- C:\path\to\3dcadcraft-mcp.exe
```

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
`extrude {distance: 10}`, `hole {face: Extrusion1:End, type: clearance, size: M8, centers: …}`,
`physical`, `project_save`. Open the file in 3DCadCraft to see it; every step is in the browser.

## Driving it by hand

One JSON-RPC message per line on stdin, answers on stdout:

```text
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"me","version":"1"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"document_new","arguments":{"kind":"part","name":"Plate"}}}
```

`tools/mcp_drive.py` sends such a script and prints the answers.
