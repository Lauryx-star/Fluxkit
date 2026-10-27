FROM mcr.microsoft.com/playwright:v1.64.1-noble@sha256:47b510dd9230bf0b9bfc9b20102241e577ea7a99334ab4c3ca19f19770e9b8ea
RUN apt-get update && apt-get install -y --no-install-recommends libnss3-tools ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /work
COPY package.json package-lock.json ./
RUN npm ci --ignore-scripts
COPY run.mjs entrypoint.sh ./
ENTRYPOINT ["/bin/sh", "/work/entrypoint.sh"]
