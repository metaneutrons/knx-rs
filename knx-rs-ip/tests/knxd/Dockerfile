# knxd as a third-party KNXnet/IP stack for the knx-rs interoperability tests.
#
# Alpine ships knxd in the "testing" repository of edge. The tests print the
# version they ran against (`knxd --version`); at the time of writing it is
# knxd 0.14.61. python3 runs the datagram relay (relay.py), tcpdump gives wire
# traces from inside the container.
FROM alpine:edge

RUN echo "https://dl-cdn.alpinelinux.org/alpine/edge/testing" >> /etc/apk/repositories \
 && apk add --no-cache knxd tini tcpdump gettext-envsubst python3

RUN addgroup -g 1000 knxd && adduser -D -u 1000 -G knxd -h /home/knxd -s /bin/sh knxd \
 && mkdir -p /run/knxd /etc/knxd && chown knxd:knxd /run/knxd /etc/knxd

COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY relay.py /usr/local/bin/relay.py
RUN chmod 0755 /usr/local/bin/entrypoint.sh /usr/local/bin/relay.py

# ${...} references in the ini are expanded from the environment at start-up.
ENV KNX_PORT=3671 KNX_HEARTBEAT=30 KNX_HEARTBEAT_RETRIES=3 KNX_RETRY_DELAY=1 KNXD_ERROR_LEVEL=6 KNXD_TRACE_MASK=0xffe

EXPOSE 3671/udp

ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/entrypoint.sh"]
USER knxd
WORKDIR /home/knxd
