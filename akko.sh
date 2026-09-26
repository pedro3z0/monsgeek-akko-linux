#!/usr/bin/env bash
#
# akko.sh — one entry point for the MonsGeek/Akko Linux driver.
#
# Wraps the three ways to use the driver — the official webapp (via the local
# gRPC helper), the interactive TUI, and plain CLI — and encodes the rules
# that are easy to get wrong:
#
#   * the official webapp (app.monsgeek.com) does not talk to the keyboard
#     itself; it needs a local helper on 127.0.0.1:3814  ->  `web` / `serve`
#   * only one interface should drive the HID device at a time, so commands
#     warn while the server is up, and `stop` takes it down again
#   * udev rules + device database must be installed before the driver can
#     open the keyboard  ->  `setup`
#
# Usage: akko <command> [args...]        (./akko.sh from a source checkout)
set -euo pipefail

# rustup installs cargo into ~/.cargo/bin, which is not on PATH in every shell
export PATH="$HOME/.cargo/bin:$PATH"

# The launcher works in two places: next to the Makefile (checkout) and in
# $BIN_DIR after `make install-launcher`. Source-tree commands (setup, rebuild
# hints) need the checkout; everything else only needs the installed driver.
find_repo() {
    if [[ -n "${AKKO_REPO:-}" \
       && -f "$AKKO_REPO/Makefile" && -d "$AKKO_REPO/iot_driver_linux" ]]; then
        echo "$AKKO_REPO"
        return 0
    fi
    local own
    own="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
    if [[ -f "$own/Makefile" && -d "$own/iot_driver_linux" ]]; then
        echo "$own"
        return 0
    fi
    return 1
}
REPO="$(find_repo || true)"
DRIVER_DIR="${REPO:+$REPO/iot_driver_linux}"
GRPC_PORT=3814
WEBAPP_URL="https://app.monsgeek.com"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}"
SERVE_PIDFILE="$RUNTIME_DIR/akko-serve.pid"
SERVE_LOG="$RUNTIME_DIR/akko-serve.log"

# ---- helpers ----------------------------------------------------------------

ok()   { printf '  [ ok ] %s\n' "$*"; }
bad()  { printf '  [ !! ] %s\n' "$*"; }
off()  { printf '  [ -- ] %s\n' "$*"; }
hint() { printf '         %s\n' "$*"; }

# Installed binary wins (what the webapp/TUI wrappers find on PATH), then the
# local release build, then the debug build left behind by `cargo test`.
find_driver() {
    local cand
    if cand="$(command -v iot_driver 2>/dev/null)"; then
        echo "$cand"
        return 0
    fi
    if [[ -n "$DRIVER_DIR" ]]; then
        for cand in "$DRIVER_DIR/target/release/iot_driver" \
                    "$DRIVER_DIR/target/debug/iot_driver"; do
            if [[ -x "$cand" ]]; then
                echo "$cand"
                return 0
            fi
        done
    fi
    return 1
}

# Is the gRPC helper (the thing the official webapp connects to) listening?
server_running() {
    if command -v ss >/dev/null 2>&1; then
        ss -ltn 2>/dev/null | awk '{print $4}' | grep -q ":$GRPC_PORT\$"
    else
        # fallback: a plain TCP probe
        if (exec 3<>"/dev/tcp/127.0.0.1/$GRPC_PORT") 2>/dev/null; then
            exec 3>&- 3<&-
            return 0
        fi
        return 1
    fi
}

# The server/TUI holds the keyboard; warn instead of blocking (the user may
# know what they are doing, and read-only commands usually still succeed).
warn_if_held() {
    if server_running; then
        echo "note: the gRPC server is running and holds the keyboard;" >&2
        echo "      if a command misbehaves, stop it first: $0 stop" >&2
    fi
}

require_driver() {
    local d
    if ! d="$(find_driver)"; then
        echo "error: no iot_driver binary found." >&2
        if [[ -n "$REPO" ]]; then
            echo "       Build and install it first: $0 setup" >&2
        else
            echo "       Build it from a checkout ('make driver && sudo make install-driver')" >&2
            echo "       or point AKKO_REPO at your checkout of this repository." >&2
        fi
        exit 1
    fi
    echo "$d"
}

cmd_help() {
    cat <<EOF
akko — MonsGeek/Akko keyboard driver, simplified

Usage: $0 <command> [args...]

Setup & health
  setup          Build the driver and install it + udev rules + device db
                 (delegates to: make driver && sudo make install; needs a
                 source checkout next to this script, or \$AKKO_REPO set)
  status         One-shot health check: binary, USB device, udev rules,
                 end-to-end query, webapp server, data files

Official webapp (app.monsgeek.com)
  web            Start the local gRPC server (127.0.0.1:$GRPC_PORT) in the
                 background and open the webapp in your browser
  serve          Run the gRPC server in the foreground (logs to stdout)
  stop           Stop a server started in the background

Interactive
  tui            Launch the terminal UI (Device Info, LED, key depth,
                 triggers, macros). Needs a real TTY.

CLI
  anything else  Passed straight to iot_driver, e.g.:
                   $0 info
                   $0 all
                   $0 triggers
                   $0 set-led wave 4 3
                   $0 --monitor info     (global flags work too)

Run '$0 status' first if something does not work.
EOF
}

cmd_setup() {
    if [[ -z "$REPO" ]]; then
        echo "error: 'setup' builds from a source checkout, which was not found" >&2
        echo "       next to this script or in \$AKKO_REPO." >&2
        echo "       Run it from the checkout (./akko.sh setup) or set:" >&2
        echo "         AKKO_REPO=/path/to/monsgeek-akko-linux $0 setup" >&2
        exit 1
    fi
    echo "==> Building driver (make driver)..."
    make -C "$REPO" driver
    echo "==> Installing driver + udev rules + device database (sudo)..."
    sudo make -C "$REPO" install
    echo "==> Done. Replug the keyboard, then check with: $0 status"
}

cmd_status() {
    echo "MonsGeek/Akko driver status"
    echo

    local driver=""
    if driver="$(find_driver)"; then
        ok "driver binary: $driver"
        local ver
        ver="$("$driver" --version 2>/dev/null || true)"
        [[ -n "$ver" ]] && ok "$ver"
    else
        bad "driver binary not built or not installed"
        if [[ -n "$REPO" ]]; then
            hint "run: $0 setup"
        else
            hint "install it from a checkout: make driver && sudo make install-driver"
        fi
    fi

    if command -v lsusb >/dev/null 2>&1; then
        if lsusb -d 3151: >/dev/null 2>&1; then
            ok "keyboard on USB bus: $(lsusb -d 3151: | head -1)"
        else
            bad "no MonsGeek/Akko device (VID 3151) on the USB bus"
            hint "check the cable or 2.4 GHz receiver"
        fi
    fi

    if [[ -r /usr/lib/udev/rules.d/99-monsgeek.rules \
       || -r /etc/udev/rules.d/99-monsgeek.rules ]]; then
        ok "udev rules installed"
    else
        bad "udev rules missing (hidraw permission denied is likely)"
        if [[ -n "$REPO" ]]; then
            hint "run: sudo make -C '$REPO' install-udev, then replug"
        else
            hint "run 'sudo make install-udev' in a checkout, then replug"
        fi
    fi

    if { [[ -n "$REPO" ]] && [[ -r "$REPO/data/device_matrices.json" ]]; } \
        || [[ -r /usr/local/share/akko/device_matrices.json ]] \
        || [[ -r /usr/share/akko/device_matrices.json ]]; then
        ok "device database present"
    else
        bad "device database (device_matrices.json) not found"
        if [[ -n "$REPO" ]]; then
            hint "run: $0 setup"
        else
            hint "run 'sudo make install-data' in a checkout"
        fi
    fi

    if [[ -n "$driver" ]]; then
        if server_running; then
            off "end-to-end query skipped — gRPC server holds the keyboard"
        else
            local out=""
            if out="$(timeout 15 "$driver" info 2>&1)"; then
                ok "keyboard answers queries"
                grep -E '^(Firmware|Device ID|Protocol):' <<<"$out" \
                    | sed 's/^/         /' || true
            else
                bad "keyboard query failed"
                hint "$(tail -1 <<<"$out")"
                hint "permissions? udev rules + replug; or another app holds it"
            fi
        fi
    fi

    echo
    if server_running; then
        ok "webapp server listening on 127.0.0.1:$GRPC_PORT — open $WEBAPP_URL"
    else
        off "webapp server not running (start it with: $0 web)"
    fi
}

cmd_serve() {
    local driver
    driver="$(require_driver)"
    if server_running; then
        echo "error: something is already listening on 127.0.0.1:$GRPC_PORT." >&2
        echo "       Stop it with: $0 stop" >&2
        exit 1
    fi
    echo "Starting gRPC server for the official webapp (Ctrl-C to end)..."
    exec "$driver" serve
}

cmd_web() {
    local driver
    driver="$(require_driver)"
    if ! server_running; then
        echo "==> Starting gRPC server in the background (log: $SERVE_LOG)"
        nohup "$driver" serve >"$SERVE_LOG" 2>&1 &
        echo $! >"$SERVE_PIDFILE"
        local i
        for i in $(seq 1 25); do
            server_running && break
            sleep 0.2
        done
        if ! server_running; then
            echo "error: server failed to start; last log lines:" >&2
            tail -5 "$SERVE_LOG" >&2
            rm -f "$SERVE_PIDFILE"
            exit 1
        fi
    fi
    ok "server ready on 127.0.0.1:$GRPC_PORT"
    echo "==> Opening $WEBAPP_URL"
    # Backgrounded on purpose: xdg-open can block until the browser exits,
    # which would wedge this command long after the page is up.
    (xdg-open "$WEBAPP_URL" >/dev/null 2>&1 &) || true
    echo "    (if no browser window appears, open $WEBAPP_URL yourself)"
    echo "    Stop the server later with: $0 stop"
}

cmd_stop() {
    local stopped=0 pid=""

    if [[ -f "$SERVE_PIDFILE" ]]; then
        pid="$(cat "$SERVE_PIDFILE" 2>/dev/null || true)"
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            stopped=1
        fi
        rm -f "$SERVE_PIDFILE"
    fi

    # Server started outside this script? Ask the kernel who owns the port.
    if server_running && command -v ss >/dev/null 2>&1; then
        pid="$(ss -ltnp "sport = :$GRPC_PORT" 2>/dev/null \
               | grep -oP 'pid=\K[0-9]+' | head -1 || true)"
        if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
            kill "$pid" 2>/dev/null || true
            stopped=1
        fi
    fi

    if [[ "$stopped" -eq 1 ]]; then
        local i
        for i in $(seq 1 20); do
            server_running || break
            sleep 0.1
        done
    fi

    if server_running; then
        bad "server is still running on port $GRPC_PORT"
        exit 1
    elif [[ "$stopped" -eq 1 ]]; then
        ok "server stopped"
    else
        echo "No server was running."
    fi
}

cmd_tui() {
    local driver
    driver="$(require_driver)"
    warn_if_held
    exec "$driver" tui
}

passthrough() {
    local driver
    driver="$(require_driver)"
    warn_if_held
    exec "$driver" "$@"
}

main() {
    local cmd="${1:-help}"
    if [[ $# -gt 0 ]]; then
        shift
    fi
    case "$cmd" in
        setup)          cmd_setup "$@" ;;
        status)         cmd_status "$@" ;;
        web)            cmd_web "$@" ;;
        serve)          cmd_serve "$@" ;;
        stop)           cmd_stop "$@" ;;
        tui)            cmd_tui "$@" ;;
        help|-h|--help) cmd_help ;;
        *)              passthrough "$cmd" "$@" ;;
    esac
}

main "$@"
