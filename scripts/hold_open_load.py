#!/usr/bin/env python3
"""
HTTP simultaneous-connection load tester.

Each "slot" maintains one persistent TCP connection and fires requests
continuously for the duration of --test-time.  When a connection is
closed by the remote side the slot immediately re-connects (as long as
time remains), so the pool stays full at all times.

CLI example:
  python hold_open_load.py --host 192.168.5.175 --port 8080 \
      -c 100 --test-time 30 --request-timeout 5

Tracked statistics
------------------
  test_duration_s          – actual elapsed test time
  connections_established  – total successful TCP connects
  connections_dropped      – connections closed / reset by the remote side
  connections_error        – TCP connect failures
  peak_simultaneous        – highest number of open connections at once
  requests_sent            – total requests attempted (ok + lost)
  requests_ok              – responses received successfully
  requests_lost            – requests that failed to receive a response
  overall_rps              – requests_ok / test_duration_s
  per_conn_rate_avg        – average req/s across all finished connections
  per_conn_rate_min        – minimum req/s across all finished connections
  per_conn_rate_max        – maximum req/s across all finished connections
"""

import argparse
import asyncio
import time

# ---------------------------------------------------------------------------
# Wire format
# ---------------------------------------------------------------------------

REQUEST_TEMPLATE = (
    "GET {path} HTTP/1.1\r\n"
    "Host: {host}:{port}\r\n"
    "Connection: keep-alive\r\n"
    "User-Agent: hold-open-load/1.0\r\n"
    "Accept: */*\r\n"
    "\r\n"
)

# ---------------------------------------------------------------------------
# Statistics
# ---------------------------------------------------------------------------

class Stats:
    """All counters are updated from async tasks; a single lock guards them."""

    def __init__(self):
        self._lock = asyncio.Lock()
        self.connections_established: int = 0
        self.connections_dropped: int = 0   # remote side sent EOF / RST
        self.connections_error: int = 0     # could not connect at all
        self.open_now: int = 0
        self.peak_open: int = 0
        self.requests_ok: int = 0
        self.requests_lost: int = 0         # sent but no valid response
        self._conn_rates: list[float] = []  # req/s per finished connection

    async def on_connect(self):
        async with self._lock:
            self.connections_established += 1
            self.open_now += 1
            if self.open_now > self.peak_open:
                self.peak_open = self.open_now

    async def on_disconnect(self, *, dropped_by_remote: bool):
        async with self._lock:
            self.open_now -= 1
            if dropped_by_remote:
                self.connections_dropped += 1

    async def on_connect_error(self):
        async with self._lock:
            self.connections_error += 1

    async def on_request_ok(self):
        async with self._lock:
            self.requests_ok += 1

    async def on_request_lost(self):
        async with self._lock:
            self.requests_lost += 1

    async def record_conn_rate(self, rate: float):
        async with self._lock:
            self._conn_rates.append(rate)

    # Read-only helpers (no lock needed for display purposes)
    def avg_conn_rate(self) -> float:
        return sum(self._conn_rates) / len(self._conn_rates) if self._conn_rates else 0.0

    def min_conn_rate(self) -> float:
        return min(self._conn_rates) if self._conn_rates else 0.0

    def max_conn_rate(self) -> float:
        return max(self._conn_rates) if self._conn_rates else 0.0

# ---------------------------------------------------------------------------
# HTTP response reader
# ---------------------------------------------------------------------------

async def read_response(reader: asyncio.StreamReader) -> str:
    """Read one complete HTTP/1.x response; return the status line."""
    raw = await reader.readuntil(b"\r\n\r\n")
    header_text = raw.decode("iso-8859-1", errors="replace")
    lines = header_text.split("\r\n")
    status_line = lines[0]
    content_length: int | None = None
    chunked = False

    for line in lines[1:]:
        if not line:
            continue
        lower = line.lower()
        if lower.startswith("content-length:"):
            try:
                content_length = int(line.split(":", 1)[1].strip())
            except ValueError:
                content_length = 0
        elif lower.startswith("transfer-encoding:") and "chunked" in lower:
            chunked = True

    if content_length is not None:
        if content_length > 0:
            await reader.readexactly(content_length)
    elif chunked:
        while True:
            size_line = await reader.readline()
            chunk_size = int(size_line.strip().split(b";", 1)[0], 16)
            if chunk_size == 0:
                await reader.readexactly(2)           # trailing CRLF
                while True:                           # optional trailers
                    trailer = await reader.readline()
                    if trailer in (b"\r\n", b""):
                        break
                break
            await reader.readexactly(chunk_size + 2) # data + CRLF

    return status_line

# ---------------------------------------------------------------------------
# Single connection lifetime
# ---------------------------------------------------------------------------

async def run_connection(
    request: bytes,
    host: str,
    port: int,
    test_end: float,
    stats: Stats,
    connect_timeout: float,
    request_timeout: float,
) -> None:
    """Open one TCP connection and send requests until test_end or remote drops it."""
    dropped_by_remote = False
    requests_this_conn = 0
    conn_start = time.perf_counter()
    writer = None

    try:
        try:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(host, port),
                timeout=connect_timeout,
            )
        except (OSError, asyncio.TimeoutError):
            await stats.on_connect_error()
            return

        await stats.on_connect()

        while time.perf_counter() < test_end:
            # --- send ---
            try:
                writer.write(request)
                await writer.drain()
            except OSError:
                dropped_by_remote = True
                await stats.on_request_lost()
                break

            # --- receive ---
            try:
                await asyncio.wait_for(read_response(reader), timeout=request_timeout)
                requests_this_conn += 1
                await stats.on_request_ok()
            except asyncio.IncompleteReadError:
                # Server closed the connection cleanly (EOF mid-response counts
                # as dropped because we still had an outstanding request).
                dropped_by_remote = True
                await stats.on_request_lost()
                break
            except (asyncio.TimeoutError, OSError, Exception):
                dropped_by_remote = True
                await stats.on_request_lost()
                break

    finally:
        elapsed = time.perf_counter() - conn_start
        if requests_this_conn > 0 and elapsed > 0:
            await stats.record_conn_rate(requests_this_conn / elapsed)

        if writer is not None:
            await stats.on_disconnect(dropped_by_remote=dropped_by_remote)
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:
                pass

# ---------------------------------------------------------------------------
# Connection slot  (continuously reconnects for the duration of the test)
# ---------------------------------------------------------------------------

async def run_slot(
    request: bytes,
    host: str,
    port: int,
    test_end: float,
    stats: Stats,
    connect_timeout: float,
    request_timeout: float,
    reconnect_delay: float,
) -> None:
    """
    One concurrency slot.  After a connection ends it immediately opens a new
    one so that the target connection count stays saturated throughout the test.
    A tiny back-off prevents hammering the host on repeated connect failures.
    """
    while time.perf_counter() < test_end:
        await run_connection(
            request, host, port, test_end, stats, connect_timeout, request_timeout
        )
        # Brief pause before reconnecting so we don't spin on instant failures.
        if time.perf_counter() < test_end:
            await asyncio.sleep(reconnect_delay)

# ---------------------------------------------------------------------------
# Live status line
# ---------------------------------------------------------------------------

async def live_display(stats: Stats, test_start: float, test_end: float) -> None:
    prev_ok = 0
    prev_t = time.perf_counter()
    while True:
        await asyncio.sleep(1.0)
        now = time.perf_counter()
        if now >= test_end:
            break
        dt = now - prev_t
        cur_ok = stats.requests_ok
        rps = (cur_ok - prev_ok) / dt if dt > 0 else 0.0
        prev_ok = cur_ok
        prev_t = now
        elapsed = now - test_start
        remaining = test_end - now
        print(
            f"\r[{elapsed:5.1f}s / -{remaining:4.1f}s]  "
            f"open={stats.open_now:4d}  peak={stats.peak_open:4d}  "
            f"estab={stats.connections_established:5d}  "
            f"dropped={stats.connections_dropped:4d}  "
            f"ok={stats.requests_ok:7d}  lost={stats.requests_lost:5d}  "
            f"rps={rps:8.1f}",
            end="",
            flush=True,
        )
    print()  # final newline

# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

async def main() -> None:
    p = argparse.ArgumentParser(
        description="HTTP simultaneous-connection load tester",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    p.add_argument("--host", required=True, help="Target host / IP")
    p.add_argument("--port", type=int, default=8080, help="Target TCP port")
    p.add_argument("--path", default="/", help="HTTP request path")
    p.add_argument(
        "-c", "--connections", type=int, default=50,
        help="Maximum simultaneous connections (= concurrency slots)",
    )
    p.add_argument(
        "--test-time", type=float, default=30.0,
        help="Total test duration in seconds; also caps each single connection lifetime",
    )
    p.add_argument(
        "--connect-timeout", type=float, default=5.0,
        help="TCP connect timeout per attempt (seconds)",
    )
    p.add_argument(
        "--request-timeout", type=float, default=10.0,
        help="Per-request read timeout (seconds)",
    )
    p.add_argument(
        "--reconnect-delay", type=float, default=0.05,
        help="Pause between reconnect attempts within a slot (seconds)",
    )
    args = p.parse_args()

    request = REQUEST_TEMPLATE.format(
        path=args.path, host=args.host, port=args.port
    ).encode("ascii")

    stats = Stats()
    test_start = time.perf_counter()
    test_end = test_start + args.test_time

    print(
        f"Target   : http://{args.host}:{args.port}{args.path}\n"
        f"Slots    : {args.connections}\n"
        f"Test time: {args.test_time}s\n"
    )

    slots = [
        asyncio.create_task(
            run_slot(
                request,
                args.host,
                args.port,
                test_end,
                stats,
                args.connect_timeout,
                args.request_timeout,
                args.reconnect_delay,
            )
        )
        for _ in range(args.connections)
    ]
    display = asyncio.create_task(live_display(stats, test_start, test_end))

    await asyncio.gather(*slots, return_exceptions=True)
    await display

    dt = time.perf_counter() - test_start
    total_req = stats.requests_ok + stats.requests_lost
    overall_rps = stats.requests_ok / dt if dt > 0 else 0.0

    print("\n=== Final Statistics " + "=" * 40)
    print(f"  test_duration_s         : {dt:.3f}")
    print(f"  connections_established : {stats.connections_established}")
    print(f"  connections_dropped     : {stats.connections_dropped}  (remote closed / reset)")
    print(f"  connections_error       : {stats.connections_error}  (failed to connect)")
    print(f"  peak_simultaneous       : {stats.peak_open}")
    print(f"  requests_sent           : {total_req}")
    print(f"  requests_ok             : {stats.requests_ok}")
    print(f"  requests_lost           : {stats.requests_lost}")
    print(f"  overall_rps             : {overall_rps:.2f}")
    if stats._conn_rates:
        print(f"  per_conn_rate_avg       : {stats.avg_conn_rate():.2f} req/s")
        print(f"  per_conn_rate_min       : {stats.min_conn_rate():.2f} req/s")
        print(f"  per_conn_rate_max       : {stats.max_conn_rate():.2f} req/s")
    print("=" * 61)


if __name__ == "__main__":
    asyncio.run(main())
