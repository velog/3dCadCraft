#!/usr/bin/env python3
"""Drive the 3DCadCraft MCP server with a script of tool calls (docs/MCP.md).

    python tools/mcp_drive.py SERVER SCRIPT.json

SERVER is the `3dcadcraft-mcp` binary; SCRIPT is a JSON list of [tool, arguments] pairs. The
script initializes, runs every call in order, prints one line per answer and stops at the first
tool error (exit code 1).
"""

import json
import subprocess
import sys


def main():
    server, script = sys.argv[1], sys.argv[2]
    with open(script, encoding="utf-8") as f:
        calls = json.load(f)
    p = subprocess.Popen([server], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
    ident = 0

    def request(method, params):
        nonlocal ident
        ident += 1
        p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": ident, "method": method, "params": params}) + "\n")
        p.stdin.flush()
        while True:
            line = p.stdout.readline()
            if not line:
                raise SystemExit("server closed")
            msg = json.loads(line)
            if msg.get("id") == ident:
                return msg
            print("  note:", line.strip())

    init = request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                  "clientInfo": {"name": "mcp_drive", "version": "1"}})
    print("initialize:", init["result"]["serverInfo"], init["result"]["protocolVersion"])
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    tools = request("tools/list", {})["result"]["tools"]
    print("tools:", len(tools))
    code = 0
    for tool, args in calls:
        r = request("tools/call", {"name": tool, "arguments": args, "_meta": {"progressToken": ident + 1}})
        res = r.get("result", {})
        text = res.get("content", [{}])[0].get("text", r.get("error"))
        print(f"{tool}: {text}")
        if res.get("isError") or "error" in r:
            code = 1
            break
    p.stdin.close()
    p.wait(timeout=30)
    sys.exit(code)


if __name__ == "__main__":
    main()
