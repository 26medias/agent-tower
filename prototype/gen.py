"""Prototype art helper: calls the OpenAI image API. Key comes from the content-gen-mcp .env file."""
import base64, json, sys, urllib.request, urllib.error

ENV = "/home/julien/Projects/content-gen-mcp/.env"


def key():
    for line in open(ENV):
        if line.startswith("OPENAI_API_KEY="):
            return line.split("=", 1)[1].strip().strip("\"'")
    raise SystemExit("no key")


def call(path, body=None):
    req = urllib.request.Request(
        "https://api.openai.com/v1/" + path,
        data=json.dumps(body).encode() if body else None,
        headers={"Authorization": "Bearer " + key(), "Content-Type": "application/json"},
    )
    try:
        return json.load(urllib.request.urlopen(req, timeout=600))
    except urllib.error.HTTPError as e:
        raise SystemExit(f"HTTP {e.code}: {e.read().decode()[:600]}")


if __name__ == "__main__":
    if sys.argv[1] == "models":
        print(sorted(m["id"] for m in call("models")["data"] if "image" in m["id"]))
    else:
        # gen.py <model> <out.png> <size> <quality> <background> <prompt>
        model, out, size, quality, background, prompt = sys.argv[1:7]
        body = {"model": model, "prompt": prompt, "size": size, "quality": quality, "n": 1}
        if background != "auto":
            body["background"] = background
        r = call("images/generations", body)
        open(out, "wb").write(base64.b64decode(r["data"][0]["b64_json"]))
        print("wrote", out, r.get("usage"))
