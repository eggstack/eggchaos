#!/usr/bin/env python3
"""Independent client smoke for the eggchaos Toxiproxy compatibility surface.

Uses only the Python standard library (urllib) against the HTTP contract:
create/populate, add latency, update latency (POST and PATCH), remove toxic,
disable/enable, reset, delete. Prints a JSON summary; exits nonzero on failure.

Usage: python3 py_smoke.py http://127.0.0.1:8474
"""
import json
import sys
import urllib.request
import urllib.error

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8474"
STEPS = []


def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            payload = resp.read().decode()
            return resp.status, (json.loads(payload) if payload else None)
    except urllib.error.HTTPError as e:
        payload = e.read().decode()
        try:
            return e.code, json.loads(payload)
        except Exception:
            return e.code, payload


def step(name, fn):
    try:
        detail = fn() or ""
        STEPS.append({"name": name, "status": "pass", "detail": str(detail)})
    except Exception as e:  # noqa: BLE001
        STEPS.append({"name": name, "status": f"FAIL: {e}"})


def main():
    status, body = call("GET", "/version")
    step("version", lambda: _check(status == 200 and body == {"version": "2.12.0"}, body))

    status, created = call("POST", "/proxies", {
        "name": "pysmoke", "listen": "127.0.0.1:0", "upstream": "127.0.0.1:1"})
    listen = created.get("listen", "") if isinstance(created, dict) else ""
    step("create", lambda: _check(status == 201 and listen, created))

    status, populated = call("POST", "/populate", [
        {"name": "pysmoke", "listen": listen, "upstream": "127.0.0.1:1", "enabled": True}])
    step("populate", lambda: _check(status == 201 and populated["proxies"][0]["name"] == "pysmoke", populated))

    status, toxic = call("POST", "/proxies/pysmoke/toxics", {
        "name": "lag", "type": "latency", "stream": "downstream",
        "toxicity": 1.0, "attributes": {"latency": 50}})
    step("add-toxic", lambda: _check(status == 200 and toxic["name"] == "lag", toxic))

    status, updated = call("POST", "/proxies/pysmoke/toxics/lag",
                           {"toxicity": 0.5, "attributes": {"latency": 100}})
    step("update-toxic-post", lambda: _check(status == 200 and updated["toxicity"] == 0.5, updated))

    status, patched = call("PATCH", "/proxies/pysmoke/toxics/lag", {"toxicity": 0.25})
    step("update-toxic-patch", lambda: _check(status == 200 and patched["toxicity"] == 0.25, patched))

    status, toxics = call("GET", "/proxies/pysmoke/toxics")
    step("list-toxics", lambda: _check(status == 200 and len(toxics) == 1, toxics))

    status, _ = call("DELETE", "/proxies/pysmoke/toxics/lag")
    step("remove-toxic", lambda: _check(status == 204, status))

    status, _ = call("POST", "/proxies/pysmoke", {"enabled": False})
    step("disable", lambda: _check(status == 200, status))

    status, _ = call("POST", "/proxies/pysmoke", {"enabled": True})
    step("enable", lambda: _check(status == 200, status))

    status, _ = call("POST", "/reset")
    step("reset", lambda: _check(status == 204, status))

    status, _ = call("DELETE", "/proxies/pysmoke")
    step("delete", lambda: _check(status == 204, status))

    failed = sum(1 for s in STEPS if s["status"] != "pass")
    print(json.dumps({"client": "python3-urllib-stdlib", "steps": STEPS,
                      "failed": failed, "overall": {"pass": failed == 0}}))
    sys.exit(1 if failed else 0)


def _check(cond, detail):
    if not cond:
        raise AssertionError(detail)
    if isinstance(detail, (dict, list)):
        return json.dumps(detail)[:160]
    return detail


main()
