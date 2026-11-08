#!/usr/bin/env python3

import json
import subprocess
import sys


def send(host, message):
    payload = json.dumps(message).encode()
    host.stdin.write(len(payload).to_bytes(4, "little") + payload)
    host.stdin.flush()


def receive(host, message_type, request_id=None):
    # Execution and cell-close notifications may interleave with responses.
    for _ in range(16):
        header = host.stdout.read(4)
        if len(header) != 4:
            raise RuntimeError("Code-mode host closed its output unexpectedly")
        size = int.from_bytes(header, "little")
        if not 0 < size <= 65536:
            raise RuntimeError(f"Unexpected smoke-test frame size: {size}")
        message = json.loads(host.stdout.read(size))
        if message.get("result", {}).get("status") == "error":
            raise RuntimeError(message)
        if message["type"] == message_type and (
            request_id is None or message.get("id") == request_id
        ):
            return message
    raise RuntimeError(f"Missing code-mode response: {message_type}")


with subprocess.Popen(
    [sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE
) as host:
    send(host, {
        "type": "connection/hello", "supportedVersions": [1],
        "requiredCapabilities": [], "optionalCapabilities": [],
    })
    assert receive(host, "connection/ready")["selectedVersion"] == 1
    send(host, {
        "type": "operation/request", "id": 1,
        "request": {"method": "session/open", "sessionId": "release-smoke"},
    })
    assert receive(host, "operation/response", 1)["result"] == {
        "status": "ok", "value": {"type": "session/ready", "sessionId": "release-smoke"},
    }
    send(host, {
        "type": "operation/request", "id": 2,
        "request": {
            "method": "session/execute", "sessionId": "release-smoke",
            "request": {
                "tool_call_id": "release-smoke", "enabled_tools": [],
                "source": "text(6 * 7);", "yield_time_ms": 10000,
                "max_output_tokens": 100,
            },
        },
    })
    result = receive(host, "execute/initialResponse", 2)["result"]["value"]["Result"]
    assert result["error_text"] is None, result
    assert result["content_items"] == [{"type": "input_text", "text": "42"}], result
    send(host, {
        "type": "operation/request", "id": 3,
        "request": {"method": "session/shutdown", "sessionId": "release-smoke"},
    })
    assert receive(host, "operation/response", 3)["result"] == {
        "status": "ok", "value": {"type": "session/closed", "sessionId": "release-smoke"},
    }
    host.stdin.close()
    assert host.wait() == 0

print("Code-mode runtime executed JavaScript successfully")
