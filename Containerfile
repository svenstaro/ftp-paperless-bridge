FROM gcr.io/distroless/cc-debian13
COPY --chmod=755 ftp-paperless-bridge /app/
ENTRYPOINT ["/app/ftp-paperless-bridge"]
