FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

# System deps for downloading/installing tools
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl git \
  && rm -rf /var/lib/apt/lists/*

# Install cargo-audit (as the vscode user so it lands in ~/.cargo/bin)
USER vscode 
RUN cargo install cargo-audit

# Install gitleaks binary into /usr/local/bin (needs root)
USER root
RUN curl -sSfL https://raw.githubusercontent.com/gitleaks/gitleaks/master/install.sh \
  | bash -s -- -b /usr/local/bin

# Back to normal
USER vscode

