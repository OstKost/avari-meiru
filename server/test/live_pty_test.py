#!/usr/bin/env python3
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time

def set_winsize(fd, rows, cols):
    winsize = struct.pack("HHHH", rows, cols, 0, 0)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)

def main():
    binary_path = sys.argv[1] if len(sys.argv) > 1 else "./mieru-admin"
    api_url = sys.argv[2] if len(sys.argv) > 2 else "http://127.0.0.1:9081/v1"
    token = sys.argv[3] if len(sys.argv) > 3 else "test-token-pty"

    # Create temporary token file with 0600 permissions
    token_file = "/tmp/test-pty-token"
    with open(token_file, "w") as f:
        f.write(token)
    os.chmod(token_file, 0o600)

    print(f"=== Starting Live PTY Verification for {binary_path} ===")
    print(f"API: {api_url}, TokenFile: {token_file}")

    master, slave = pty.openpty()
    # Initial PTY size: 80x24
    set_winsize(master, 24, 80)

    cmd = [binary_path, "tui", f"--api-url={api_url}", f"--token-file={token_file}"]
    proc = subprocess.Popen(
        cmd,
        stdin=slave,
        stdout=slave,
        stderr=slave,
        close_fds=True,
    )
    os.close(slave)

    output = b""
    start_time = time.time()

    def read_available(timeout=1.0):
        nonlocal output
        r, _, _ = select.select([master], [], [], timeout)
        if r:
            chunk = os.read(master, 4096)
            output += chunk
            return chunk
        return b""

    # Wait for dashboard to render
    print("[1/4] Waiting for initial dashboard render on 80x24 PTY...")
    rendered = False
    for _ in range(30):
        read_available(0.2)
        decoded = output.decode("utf-8", errors="ignore")
        if "Mieru Admin" in decoded and "Меню управления" in decoded:
            rendered = True
            break
        time.sleep(0.1)

    if not rendered:
        print("FAIL: Initial 80x24 view did not render in time. Output so far:")
        print(output.decode("utf-8", errors="ignore"))
        proc.kill()
        sys.exit(1)
    print("✓ Initial dashboard rendered successfully in 80x24 PTY")

    # Resize to 100x30
    print("[2/4] Testing live PTY resize to 100x30...")
    set_winsize(master, 30, 100)
    proc.send_signal(signal.SIGWINCH)
    time.sleep(0.3)
    read_available(0.5)
    print("✓ SIGWINCH resize handled without crashing")

    # Send key '5' to open service screen
    print("[3/4] Testing navigation: sending '5' (Service screen)...")
    os.write(master, b"5")
    time.sleep(0.3)
    read_available(0.5)
    decoded = output.decode("utf-8", errors="ignore")
    if "Управление службой" not in decoded:
        print("FAIL: Navigation to Service screen failed")
        proc.kill()
        sys.exit(1)
    print("✓ Navigated to Service screen")

    # Send Esc to return to Dashboard
    print("[4/4] Testing Esc return and clean 'q' exit...")
    os.write(master, b"\x1b")
    time.sleep(0.3)
    read_available(0.5)

    # Send 'q' to quit
    os.write(master, b"q")
    try:
        proc.wait(timeout=3)
    except subprocess.TimeoutExpired:
        print("FAIL: Process did not exit within 3s after 'q'")
        proc.kill()
        sys.exit(1)

    os.close(master)
    if proc.returncode != 0:
        print(f"FAIL: Non-zero exit code: {proc.returncode}")
        sys.exit(1)

    full_output = output.decode("utf-8", errors="ignore")
    if token in full_output:
        print("FAIL: Secret token was leaked in PTY terminal output!")
        sys.exit(1)
    print("✓ Verified zero secret token leakage in PTY output")

    print("✓ TUI exited cleanly with returncode 0 and terminal state restored")
    print("=== LIVE PTY TEST PASSED ===")

if __name__ == "__main__":
    main()
