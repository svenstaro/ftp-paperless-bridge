FROM docker.io/ubuntu
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --chmod=755 ftp-paperless-bridge /app/
ENTRYPOINT ["/app/ftp-paperless-bridge"]
