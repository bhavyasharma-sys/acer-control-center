"""Entry point: python3 -m webgui.acercontrol [--host H] [--port P]."""

from __future__ import annotations

import argparse

from . import server


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="acer-control-center-web",
        description="Acer Control Center browser frontend (standard library only).",
    )
    parser.add_argument(
        "--host",
        default="127.0.0.1",
        help="interface to bind (default 127.0.0.1; use 0.0.0.0 to share)",
    )
    parser.add_argument("--port", type=int, default=8765)
    options = parser.parse_args()

    httpd = server.serve(options.host, options.port)
    print(f"Acer Control Center on http://{options.host}:{options.port}/")
    print("Press Ctrl+C to stop.")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()
