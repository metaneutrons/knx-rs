#!/usr/bin/env python3
"""Datagram relay: length-prefixed UDP datagrams on stdin/stdout <-> UDP.

Usage: relay.py HOST PORT

Every datagram read from stdin (2-byte big-endian length, then the payload) is
sent to HOST:PORT from one UDP socket. Every datagram that socket receives is
written to stdout in the same framing. The test harness runs this through
`docker exec -i`, which works on every Docker setup, also where published UDP
ports do not reach the host.
"""
import os
import socket
import struct
import sys
import threading

target = (sys.argv[1], int(sys.argv[2]))
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(("127.0.0.1", 0))


def pump_stdin():
    stdin = sys.stdin.buffer
    while True:
        header = stdin.read(2)
        if len(header) < 2:
            os._exit(0)
        (length,) = struct.unpack(">H", header)
        payload = stdin.read(length)
        if len(payload) < length:
            os._exit(0)
        sock.sendto(payload, target)


threading.Thread(target=pump_stdin, daemon=True).start()
stdout = sys.stdout.buffer
while True:
    data, _ = sock.recvfrom(2048)
    stdout.write(struct.pack(">H", len(data)) + data)
    stdout.flush()
