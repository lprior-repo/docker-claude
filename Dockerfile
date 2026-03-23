FROM archlinux:latest AS builder

RUN pacman -Syu --noconfirm && \
    pacman -S --noconfirm \
    base-devel \
    cmake \
    curl \
    git \
    perl \
    llvm \
    && pacman -Scc --noconfirm

ENV RUSTUP_VERSION=1.27.0
RUN curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init && \
    chmod +x /tmp/rustup-init && \
    sh /tmp/rustup-init -y --default-toolchain stable --profile minimal && \
    rm /tmp/rustup-init
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup component add llvm-tools-preview

RUN cargo install --locked cargo-expand && \
    cargo install --locked cargo-audit && \
    cargo install --locked cargo-flamegraph && \
    cargo install --locked cargo-watch && \
    cargo install --locked cargo-outdated && \
    cargo install --locked cargo-mutants && \
    cargo install --locked cargo-llvm-cov && \
    cargo install --locked cargo-geiger && \
    cargo install --locked tokei && \
    cargo install --locked rust-code-analysis-cli && \
    cargo install --locked cargo-udeps && \
    cargo install --locked cargo-deny && \
    cargo install --locked ast-grep && \
    cargo install --locked sg && \
    cargo install --locked wasm-pack || \
    echo "WARNING: Some cargo tools failed to install (non-critical)"

FROM archlinux:latest
LABEL maintainer="claude-dock"
LABEL description="Bleeding-edge Claude Code CLI on Arch + Bun + Rust + Python + JJ + Dioxus"
LABEL claude-dock="true"

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
    python \
    python-pip \
    python-setuptools \
    python-wheel \
    icu \
    dolt \
    gnupg \
    pinentry \
    gosu \
    && pacman -Scc --noconfirm

RUN curl -LsSf https://astral.sh/uv/install.sh | sh
ENV PATH="/root/.local/bin:/root/.cargo/bin:/root/go/bin:${PATH}"

RUN uv --version && python3 --version

ENV DELTA_VERSION=0.18.1
RUN ARCH=$(uname -m) && \
    case "$ARCH" in \
        x86_64) DELTA_ARCH=x86_64-unknown-linux-musl ;; \
        aarch64) DELTA_ARCH=aarch64-unknown-linux-musl ;; \
        *) echo "Unsupported architecture: $ARCH" && exit 1 ;; \
    esac && \
    curl -fsSL "https://github.com/dandavison/delta/releases/download/$DELTA_VERSION/delta-$DELTA_VERSION-$DELTA_ARCH.tar.gz" -o /tmp/delta.tar.gz && \
    tar -xzf /tmp/delta.tar.gz -C /tmp && \
    mv /tmp/delta-$DELTA_VERSION-$DELTA_ARCH/delta /usr/local/bin/ && \
    chmod +x /usr/local/bin/delta && \
    rm -rf /tmp/delta.tar.gz /tmp/delta-$DELTA_VERSION-$DELTA_ARCH

COPY --from=builder /root/.cargo/bin /root/.cargo/bin
COPY --from=builder /root/.rustup /root/.rustup
RUN rustc --version && cargo --version

RUN npm install -g playwright && \
    npx playwright install chromium

RUN gh extension install __CHALK__/gh-delta || \
    echo "WARNING: gh-delta extension failed to install (non-critical)"

RUN curl -fsSL https://raw.githubusercontent.com/steveyegge/beads/main/scripts/install.sh | bash || \
    (echo "beads install script failed, trying go install..." && \
     CGO_ENABLED=1 go install github.com/steveyegge/beads/cmd/bd@latest)
RUN bd --version || beads --version || \
    (echo "ERROR: bd/beads installation failed" && exit 1)

ENV DIOXUS_AGENT_DIR="/opt/dioxus-agent-rs"
RUN git clone --depth 1 https://github.com/nicholasoller/dioxus-agent-rs "$DIOXUS_AGENT_DIR" && \
    cd "$DIOXUS_AGENT_DIR" && \
    cargo build --release && \
    mv target/release/dioxus-agent-rs /usr/local/bin/ && \
    chmod +x /usr/local/bin/dioxus-agent-rs && \
    cd / && rm -rf "$DIOXUS_AGENT_DIR" || \
    echo "WARNING: dioxus-agent-rs failed to build (non-critical, Dioxus CDP features disabled)"

RUN npm install -g \
    typescript-language-server \
    typescript \
    @tailwindcss/language-server \
    pyright && \
    rustup component add rust-analyzer && \
    go install golang.org/x/tools/gopls@latest

RUN curl -fsSL https://claude.ai/install.sh | bash && \
    mv /root/.local/share/claude /usr/local/share/claude && \
    rm /root/.local/bin/claude && \
    ln -s /usr/local/share/claude/versions/$(ls /usr/local/share/claude/versions | head -n 1) /usr/local/bin/claude && \
    claude --version

RUN mkdir -p /app /home/user/.ssh /home/user/.config /home/user/.local /home/user/.cache /home/user/.gnupg && \
    chown -R root:root /home/user && \
    chmod 755 /home/user

WORKDIR /app
COPY target/release/claude-dock /usr/local/bin/claude-dock
RUN chmod +x /usr/local/bin/claude-dock

ENTRYPOINT ["/usr/local/bin/claude-dock", "__entrypoint"]
