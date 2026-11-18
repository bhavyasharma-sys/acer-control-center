"""Acer Control Center, browser frontend.

A dependency-free Python backend (standard library only) that talks to the
running acer-control-daemon over D-Bus through `busctl`, plus telemetry read
straight from /proc and /sys. The UI is plain HTML/CSS/JS served by this
package, so the whole frontend runs anywhere Python 3 runs, with nothing to
compile.
"""

__version__ = "2.2.0"
