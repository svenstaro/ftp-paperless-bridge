# ftp-paperless-bridge

Present a FTP server to your network scanner and forward anything received to paperless-ngx

## Run

```shell
cp .env.example .env
# Fill out .env with your info
podman run --init -it --env-file .env ghcr.io/svenstaro/ftp-paperless-bridge:latest
```

## Develop

```shell
cp .env.example .env
# Fill out .env with your info
just run
```
