FROM archlinux:latest
LABEL maintainer="claude-dock"
LABEL description="Bleeding-edge Claude Code CLI on Arch + Bun"

# 1. Sync and install the absolute latest native binaries.
RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm \
    bun \
    git \
    ripgrep \
    fzf \
    bat \
    unzip \
    curl \
    ca-certificates \
    && pacman -Scc --noconfirm

# Install gosu manually
ENV GOSU_VERSION=1.17
RUN ARCH=$(uname -m) && \
    case "$ARCH" in \
        x86_64) GOSU_ARCH=amd64 ;; \
        aarch64) GOSU_ARCH=arm64 ;; \
        *) echo "Unsupported architecture: $ARCH" && exit 1 ;; \
    esac && \
    curl -L "https://github.com/tianon/gosu/releases/download/$GOSU_VERSION/gosu-$GOSU_ARCH" -o /usr/local/bin/gosu && \
    chmod +x /usr/local/bin/gosu

# 2. THE PERFORMANCE HACK: 
RUN ln -s /usr/bin/bun /usr/bin/node

# 3. Install Claude Code using the official install script
# We move it to /usr/local to make it available to all users
RUN curl -fsSL https://claude.ai/install.sh | bash && \
    mv /root/.local/share/claude /usr/local/share/claude && \
    rm /root/.local/bin/claude && \
    ln -s /usr/local/share/claude/versions/$(ls /usr/local/share/claude/versions | head -n 1) /usr/local/bin/claude

# 4. Setup directories for host volume mounts
RUN mkdir -p /app /home/user && \
    chmod -R 777 /home/user

WORKDIR /app
COPY target/release/claude-dock /usr/local/bin/claude-dock
RUN chmod +x /usr/local/bin/claude-dock

ENTRYPOINT ["/usr/local/bin/claude-dock", "__entrypoint"]
