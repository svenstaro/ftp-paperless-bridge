FROM docker.io/ubuntu
COPY --chmod=755 ftp-paperless-bridge /app/
ENTRYPOINT ["/app/ftp-paperless-bridge"]
