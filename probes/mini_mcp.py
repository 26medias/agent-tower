#!/usr/bin/env python3
# Minimal MCP server for probes (no SDK). Two transports:
#   mini_mcp.py stdio            -- newline-delimited JSON-RPC on stdin/stdout
#   mini_mcp.py http <port>      -- Streamable HTTP, POST /mcp, JSON responses (no SSE)
# Tools: whoami (shows the identity the server sees), wait(seconds) (blocks, then returns).
# Every request is logged to logs/mcp.jsonl with the identity seen.
import json
import os
import sys
import time
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LOG = "/tmp/agent-tower-probes/logs/mcp.jsonl"
lock = threading.Lock()


def log(entry):
    entry["t"] = round(time.time(), 3)
    with lock, open(LOG, "a") as f:
        f.write(json.dumps(entry) + "\n")


TOOLS = [
    {"name": "whoami", "description": "Return the caller identity that the server sees.",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "wait", "description": "Block for the given number of seconds, then return.",
     "inputSchema": {"type": "object", "properties": {"seconds": {"type": "number"}},
                     "required": ["seconds"]}},
]


def handle(msg, ident):
    method = msg.get("method")
    mid = msg.get("id")
    log({"dir": "in", "method": method, "id": mid, "ident": ident,
         "params": msg.get("params") if method != "initialize" else {
             "protocolVersion": msg.get("params", {}).get("protocolVersion"),
             "clientInfo": msg.get("params", {}).get("clientInfo")}})
    if mid is None:
        return None
    if method == "initialize":
        pv = msg.get("params", {}).get("protocolVersion", "2025-06-18")
        res = {"protocolVersion": pv, "capabilities": {"tools": {}},
               "serverInfo": {"name": "agent-tower-probe", "version": "0.1"}}
    elif method == "tools/list":
        res = {"tools": TOOLS}
    elif method == "tools/call":
        name = msg["params"]["name"]
        args = msg["params"].get("arguments") or {}
        if name == "whoami":
            text = json.dumps(ident)
        elif name == "wait":
            s = float(args.get("seconds", 1))
            t0 = time.time()
            time.sleep(s)
            text = f"woke after {round(time.time() - t0, 1)} s"
            log({"dir": "out", "method": "wait-done", "id": mid, "ident": ident, "text": text})
        else:
            return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32602, "message": "unknown tool"}}
        res = {"content": [{"type": "text", "text": text}], "isError": False}
    elif method == "ping":
        res = {}
    else:
        return {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "method not found"}}
    return {"jsonrpc": "2.0", "id": mid, "result": res}


def run_stdio():
    ident = {"transport": "stdio", "AT_TOKEN": os.environ.get("AT_TOKEN"), "pid": os.getpid(),
             "argv": sys.argv[2:]}
    out_lock = threading.Lock()

    def work(m):
        r = handle(m, ident)
        if r is not None:
            with out_lock:
                sys.stdout.write(json.dumps(r) + "\n")
                sys.stdout.flush()

    for line in sys.stdin:
        if not line.strip():
            continue
        m = json.loads(line)
        threading.Thread(target=work, args=(m,), daemon=True).start()


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        log({"dir": "in", "method": "HTTP GET", "path": self.path})
        self.send_response(405)
        self.end_headers()

    def do_DELETE(self):
        self.send_response(200)
        self.end_headers()

    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        ident = {"transport": "http", "path": self.path,
                 "authorization": self.headers.get("Authorization"),
                 "x_at_token": self.headers.get("X-AT-Token"),
                 "session": self.headers.get("Mcp-Session-Id")}
        msgs = body if isinstance(body, list) else [body]
        outs = [r for r in (handle(m, ident) for m in msgs) if r is not None]
        if not outs:
            self.send_response(202)
            self.end_headers()
            return
        data = json.dumps(outs if isinstance(body, list) else outs[0]).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        if any(m.get("method") == "initialize" for m in msgs):
            self.send_header("Mcp-Session-Id", f"s-{time.time_ns()}")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


if __name__ == "__main__":
    if sys.argv[1] == "stdio":
        run_stdio()
    else:
        ThreadingHTTPServer(("127.0.0.1", int(sys.argv[2])), H).serve_forever()
