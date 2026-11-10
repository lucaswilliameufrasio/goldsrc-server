FROM ubuntu:24.04

LABEL org.opencontainers.image.source="https://github.com/lucaswilliameufrasio/goldsrc-server"
LABEL org.opencontainers.image.description="GoldSrc query-only Rust PoC; client sign-on and gameplay are disabled"

COPY --chown=65532:65532 goldsrc-server /usr/local/bin/goldsrc-server

USER 65532:65532
EXPOSE 27025/udp
ENTRYPOINT ["/usr/local/bin/goldsrc-server"]
