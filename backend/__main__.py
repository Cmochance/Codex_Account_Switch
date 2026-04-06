from __future__ import annotations

import os

import uvicorn


def main() -> None:
    host = os.environ.get("CODEX_BACKEND_HOST", "127.0.0.1")
    port = int(os.environ.get("CODEX_BACKEND_PORT", "8765"))
    print(f"Codex control panel backend listening on http://{host}:{port}")
    uvicorn.run("backend.api:app", host=host, port=port, reload=False)


if __name__ == "__main__":
    main()

