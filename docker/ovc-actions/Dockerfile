# OVC Actions — All-in-one multi-language CI image
#
# Contains toolchains for: Rust, Go, Node.js, Python, Ruby, Java, C/C++,
# Kotlin (via Gradle), C# (.NET), Deno, Dart, Swift, PHP, and Elixir.
#
# Base: Debian bookworm-slim (glibc required by Swift, .NET, and JDK).
# Estimated uncompressed size: ~2.5 GB.

# ---- Stage 1: Rust toolchain ----
FROM rust:1.85-slim-bookworm AS rust-stage
RUN rustup component add clippy rustfmt

# ---- Stage 2: Go toolchain ----
FROM golang:1.24-bookworm AS go-stage

# ---- Final runtime stage ----
FROM debian:bookworm-slim AS runtime

# Prevent interactive prompts during package installation.
ENV DEBIAN_FRONTEND=noninteractive

# System packages: essentials, build tools, language runtimes available via apt.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    make \
    cmake \
    gcc \
    g++ \
    pkg-config \
    libssl-dev \
    # Ruby
    ruby \
    ruby-dev \
    # Java (headless JDK for builds)
    openjdk-17-jdk-headless \
    # Maven
    maven \
    # PHP
    php-cli \
    # Elixir (includes Erlang)
    elixir \
    # Utilities
    unzip \
    gnupg2 \
    apt-transport-https \
    && rm -rf /var/lib/apt/lists/*

# ---- Copy Rust from builder stage ----
COPY --from=rust-stage /usr/local/cargo /usr/local/cargo
COPY --from=rust-stage /usr/local/rustup /usr/local/rustup
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo
ENV PATH="/usr/local/cargo/bin:${PATH}"

# ---- Copy Go from builder stage ----
COPY --from=go-stage /usr/local/go /usr/local/go
ENV PATH="/usr/local/go/bin:${PATH}"

# ---- Node.js (LTS 20 via NodeSource) ----
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g eslint

# ---- Python + pip + linting/testing tools ----
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 \
    python3-pip \
    python3-venv \
    && pip3 install --no-cache-dir --break-system-packages ruff pytest \
    && rm -rf /var/lib/apt/lists/*

# ---- RuboCop ----
RUN gem install rubocop --no-document

# ---- PHP Composer ----
RUN curl -fsSL https://getcomposer.org/installer | php -- --install-dir=/usr/local/bin --filename=composer

# ---- .NET SDK 8.0 ----
RUN curl -fsSL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh \
    && chmod +x /tmp/dotnet-install.sh \
    && /tmp/dotnet-install.sh --channel 8.0 --install-dir /usr/share/dotnet \
    && ln -s /usr/share/dotnet/dotnet /usr/local/bin/dotnet \
    && rm /tmp/dotnet-install.sh
ENV DOTNET_ROOT=/usr/share/dotnet

# ---- Deno ----
RUN curl -fsSL https://deno.land/install.sh | DENO_INSTALL=/usr/local sh

# ---- Dart SDK (headless — no Flutter) ----
# Dart publishes amd64 and arm64 debs. Detect architecture dynamically.
RUN ARCH=$(dpkg --print-architecture) \
    && curl -fsSL https://dl-ssl.google.com/linux/linux_signing_key.pub \
    | gpg --dearmor -o /usr/share/keyrings/dart.gpg \
    && echo "deb [signed-by=/usr/share/keyrings/dart.gpg arch=${ARCH}] https://storage.googleapis.com/download.dartlang.org/linux/debian stable main" \
    > /etc/apt/sources.list.d/dart_stable.list \
    && apt-get update && apt-get install -y --no-install-recommends dart \
    && rm -rf /var/lib/apt/lists/* \
    || echo "Dart SDK installation skipped (arch ${ARCH} may not be supported)"
ENV PATH="/usr/lib/dart/bin:${PATH}"

# ---- Swift (swift.org Linux toolchain) ----
# Swift publishes toolchains for amd64 (x86_64) and arm64 (aarch64).
RUN SWIFT_VERSION=6.0.3 \
    && ARCH=$(uname -m) \
    && if [ "$ARCH" = "x86_64" ]; then \
         SWIFT_PLATFORM="ubuntu2204"; \
         SWIFT_ARCH=""; \
       elif [ "$ARCH" = "aarch64" ]; then \
         SWIFT_PLATFORM="ubuntu2204-aarch64"; \
         SWIFT_ARCH="-aarch64"; \
       else \
         echo "Swift: unsupported architecture $ARCH, skipping"; exit 0; \
       fi \
    && SWIFT_URL="https://download.swift.org/swift-${SWIFT_VERSION}-release/${SWIFT_PLATFORM}/swift-${SWIFT_VERSION}-RELEASE/swift-${SWIFT_VERSION}-RELEASE-ubuntu22.04${SWIFT_ARCH}.tar.gz" \
    && curl -fsSL "${SWIFT_URL}" | tar -xz -C /usr/local --strip-components=2 \
    || echo "Swift installation skipped (download may have failed)"

# ---- Gradle (Kotlin build tool) ----
RUN GRADLE_VERSION=8.7 \
    && curl -fsSL "https://services.gradle.org/distributions/gradle-${GRADLE_VERSION}-bin.zip" \
    -o /tmp/gradle.zip \
    && unzip -q /tmp/gradle.zip -d /usr/local \
    && ln -s "/usr/local/gradle-${GRADLE_VERSION}/bin/gradle" /usr/local/bin/gradle \
    && rm /tmp/gradle.zip

# ---- Workspace setup ----
WORKDIR /workspace

# Non-root user for security.
RUN useradd -ms /bin/sh ovc \
    && chown ovc:ovc /workspace
USER ovc

# Smoke-test: verify key tools are on PATH.
RUN echo "=== Tool verification ===" \
    && cargo --version \
    && go version \
    && node --version \
    && python3 --version \
    && ruby --version \
    && java -version 2>&1 \
    && dotnet --version \
    && deno --version \
    && php --version | head -1 \
    && mix --version 2>&1 | tail -1 \
    && composer --version 2>&1 | head -1 \
    && gradle --version | head -1 \
    && (dart --version 2>&1 || echo "dart: not available on this arch") \
    && (swift --version 2>&1 || echo "swift: not available on this arch") \
    && echo "=== Verification complete ==="
