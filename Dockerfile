FROM debian:bookworm-slim

LABEL maintainer="claude-dock"
LABEL description="Claude Code CLI container - Rust entrypoint"

RUN apt-get update && apt-get install -y \
    nodejs \
    npm \
    curl \
    gosu \
    git \
    vim \
    wget \
    zip \
    unzip \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code

RUN mkdir -p /app /root/.claude /root/.jj /home/npm-global

WORKDIR /app

COPY entrypoint-bin /entrypoint
RUN chmod +x /entrypoint

ENTRYPOINT ["/entrypoint"]
