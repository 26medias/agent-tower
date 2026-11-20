"""Prototype art helper: edits an image with the OpenAI image API (keeps the style of the input)."""
import base64, sys, requests
from gen import key

# edit.py <model> <in.png> <out.png> <size> <quality> <prompt>
model, src, out, size, quality, prompt = sys.argv[1:7]
r = requests.post(
    "https://api.openai.com/v1/images/edits",
    headers={"Authorization": "Bearer " + key()},
    files={"image": open(src, "rb")},
    data={"model": model, "prompt": prompt, "size": size, "quality": quality, "background": "transparent"},
    timeout=600,
)
if r.status_code != 200:
    raise SystemExit(f"HTTP {r.status_code}: {r.text[:600]}")
open(out, "wb").write(base64.b64decode(r.json()["data"][0]["b64_json"]))
print("wrote", out)
