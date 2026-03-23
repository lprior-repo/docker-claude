FROM archlinux:latest
LABEL maintainer="claude-dock"
LABEL description="Bleeding-edge Claude Code CLI on Arch + Bun + Rust + JJ + Dioxus"

# =============================================================================
# STEP 1: Base system packages (Arch Linux native)
# =============================================================================
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
    base-devel \
    cmake \
    perl \
    jq \
    zsh \
    chromium \
    github-cli \
    eza \
    fd \
    jujutsu \
    nodejs \
    npm \
    go \
    icu \
    dolt \
    && pacman -Scc --noconfirm

# =============================================================================
# STEP 2: delta & gosu
# =============================================================================
ENV DELTA_VERSION=0.18.1
ENV GOSU_VERSION=1.17
RUN ARCH=$(uname -m) && \
    case "$ARCH" in \
        x86_64) DELTA_ARCH=x86_64-unknown-linux-musl; GOSU_ARCH=amd64 ;; \
        aarch64) DELTA_ARCH=aarch64-unknown-linux-musl; GOSU_ARCH=arm64 ;; \
        *) echo "Unsupported architecture: $ARCH" && exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/dandavison/delta/releases/download/$DELTA_VERSION/delta-$DELTA_VERSION-$DELTA_ARCH.tar.gz" -o /tmp/delta.tar.gz && \
    tar -xzf /tmp/delta.tar.gz -C /tmp && \
    mv /tmp/delta-$DELTA_VERSION-$DELTA_ARCH/delta /usr/local/bin/ && \
    chmod +x /usr/local/bin/delta && \
    rm -rf /tmp/delta.tar.gz /tmp/delta-$DELTA_VERSION-$DELTA_ARCH && \
    delta --version && \
    curl -fsSL "https://github.com/tianon/gosu/releases/download/$GOSU_VERSION/gosu-$GOSU_ARCH" -o /usr/local/bin/gosu && \
    chmod +x /usr/local/bin/gosu && \
    gosu --version

# =============================================================================
# STEP 3: Rust toolchain via rustup
# =============================================================================
ENV RUSTUP_VERSION=1.27.0
RUN curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init && \
    chmod +x /tmp/rustup-init && \
    sh /tmp/rustup-init -y --default-toolchain stable --profile minimal && \
    rm /tmp/rustup-init
ENV PATH="/root/.cargo/bin:/root/go/bin:${PATH}"
RUN rustc --version && cargo --version

# =============================================================================
# STEP 4: Pre-build common cargo tools (optional - speeds up later builds)
# =============================================================================
RUN cargo install --locked cargo-expand && \
    cargo install --locked cargo-audit && \
    cargo install --locked cargo-flamegraph && \
    cargo install --locked cargo-watch && \
    cargo install --locked cargo-outdated && \
    cargo install --locked wasm-pack || \
    echo "WARNING: Some cargo tools failed to install (non-critical)"

# =============================================================================
# STEP 5: Playwright with Chromium
# NOTE: Chromium browser installed separately via npm (pacman chromium is for system use)
# =============================================================================
RUN npm install -g playwright && \
    npx playwright install chromium

# =============================================================================
# STEP 6: GitHub CLI extensions
# =============================================================================
RUN gh extension install __CHALK__/gh-delta || \
    echo "WARNING: gh-delta extension failed to install (non-critical)"

# =============================================================================
# STEP 7: beads (Steve Yegge's bd project) - issue tracker
# REQUIRED: bd/beads is needed for issue tracking
# =============================================================================
RUN curl -fsSL https://raw.githubusercontent.com/steveyegge/beads/main/scripts/install.sh | bash || \
    (echo "beads install script failed, trying go install..." && \
     CGO_ENABLED=1 go install github.com/steveyegge/beads/cmd/bd@latest)
RUN bd --version || beads --version || \
    (echo "ERROR: bd/beads installation failed" && exit 1)

# =============================================================================
# STEP 8: dioxus-agent-rs (Dioxus CDP debugging tool)
# OPTIONAL: Only needed if using Dioxus CDP features
# =============================================================================
ENV DIOXUS_AGENT_DIR="/opt/dioxus-agent-rs"
RUN git clone --depth 1 https://github.com/nicholasoller/dioxus-agent-rs "$DIOXUS_AGENT_DIR" && \
    cd "$DIOXUS_AGENT_DIR" && \
    cargo build --release && \
    mv target/release/dioxus-agent-rs /usr/local/bin/ && \
    chmod +x /usr/local/bin/dioxus-agent-rs && \
    cd / && rm -rf "$DIOXUS_AGENT_DIR" || \
    echo "WARNING: dioxus-agent-rs failed to build (non-critical, Dioxus CDP features disabled)"

# =============================================================================
# STEP 9: LSPs for Claude diagnostics
# - rust-analyzer: Rust language server
# - typescript-language-server: TypeScript/JavaScript
# - pyright: Python
# - gopls: Go
# - @tailwindcss/language-server: Tailwind CSS
# =============================================================================
RUN npm install -g \
    typescript-language-server \
    typescript \
    @tailwindcss/language-server \
    pyright && \
    rustup component add rust-analyzer && \
    go install golang.org/x/tools/gopls@latest

# =============================================================================
# STEP 10: Claude Code CLI
# =============================================================================
RUN curl -fsSL https://claude.ai/install.sh | bash && \
    mv /root/.local/share/claude /usr/local/share/claude && \
    rm /root/.local/bin/claude && \
    ln -s /usr/local/share/claude/versions/$(ls /usr/local/share/claude/versions | head -n 1) /usr/local/bin/claude && \
    claude --version

# =============================================================================
# STEP 11: Setup directories for host volume mounts
# =============================================================================
RUN mkdir -p /app /home/user && \
    chmod -R 777 /home/user

# =============================================================================
# STEP 12: Install claude-dock binary
# =============================================================================
WORKDIR /app
COPY target/release/claude-dock /usr/local/bin/claude-dock
RUN chmod +x /usr/local/bin/claude-dock

ENTRYPOINT ["/usr/local/bin/claude-dock", "__entrypoint"]
